#!/usr/bin/env python3
"""Non-mergeable macOS resource baseline. Max 64 peers; own daemon only."""
import argparse
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import resource
import signal
import socket
import statistics
import struct
import subprocess
import tempfile
import threading
import time

ROOT = Path(os.environ.get('EVIDENCE_ROOT', str(Path(__file__).resolve().parent / 'rerun-results'))).resolve()
if not os.environ.get('SOURCE_ROOT'):
    raise SystemExit('Set SOURCE_ROOT to an unchanged checkout of the recorded public Seyal revision.')
SOURCE = Path(os.environ['SOURCE_ROOT']).resolve()
BINARY = Path(os.environ.get('SEYAL_AGENT_BACKEND', str(ROOT / 'target/release/seyal-agent-backend'))).resolve()
MAX_PEERS = 64
HELLO_EVIDENCE = b'observer'  # Legacy public token; NOT production authentication proof.

def command(args, cwd=None):
    p = subprocess.run(args, cwd=cwd, text=True, capture_output=True, timeout=15)
    return {'args': args, 'returncode': p.returncode, 'stdout': p.stdout.strip(), 'stderr': p.stderr.strip()}

class TaskInfo(ctypes.Structure):
    _fields_ = [(x, ctypes.c_uint64) for x in ('virtual_size', 'resident_size', 'total_user', 'total_system', 'threads_user', 'threads_system')] + [(x, ctypes.c_int32) for x in ('policy', 'faults', 'pageins', 'cow_faults', 'messages_sent', 'messages_received', 'syscalls_mach', 'syscalls_unix', 'csw', 'threadnum', 'numrunning', 'priority')]

LIB = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
LIB.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_int]
LIB.proc_pidinfo.restype = ctypes.c_int
class Timebase(ctypes.Structure):
    _fields_ = [('numer', ctypes.c_uint32), ('denom', ctypes.c_uint32)]
TIMEBASE = Timebase()
SYSTEM = ctypes.CDLL('/usr/lib/libSystem.B.dylib')
if SYSTEM.mach_timebase_info(ctypes.byref(TIMEBASE)) != 0:
    raise RuntimeError('mach_timebase_info failed')

def metrics(pid):
    info = TaskInfo()
    n = LIB.proc_pidinfo(pid, 4, 0, ctypes.byref(info), ctypes.sizeof(info))
    if n != ctypes.sizeof(info):
        raise OSError(ctypes.get_errno(), f'PROC_PIDTASKINFO returned {n} of {ctypes.sizeof(info)}')
    buf = ctypes.create_string_buffer(8 * 2048)
    n = LIB.proc_pidinfo(pid, 1, 0, buf, len(buf))
    if n <= 0:
        raise OSError(ctypes.get_errno(), f'PROC_PIDLISTFDS returned {n}')
    return {'monotonic_ns': time.monotonic_ns(), 'rss_bytes': info.resident_size,
            'virtual_bytes': info.virtual_size, 'threads': info.threadnum,
            'fd_count': n // 8, 'cpu_total_mach_ticks': info.total_user + info.total_system,
            'cpu_total_ns': (info.total_user + info.total_system) * TIMEBASE.numer / TIMEBASE.denom}

def samples(pid, count=3, interval=.05):
    result = []
    for i in range(count):
        if i: time.sleep(interval)
        result.append(metrics(pid))
    return result

def frame(kind, body):
    return b'AGB1' + struct.pack('<HI', kind, len(body)) + body

HELLO = frame(1, struct.pack('<HHIIH', 1, 1, 65536, 256, len(HELLO_EVIDENCE)) + HELLO_EVIDENCE)

def read_exact(s, n):
    data = bytearray()
    while len(data) < n:
        b = s.recv(n - len(data))
        if not b: raise EOFError('peer closed')
        data.extend(b)
    return bytes(data)

def receive(s):
    h = read_exact(s, 10)
    magic, kind, n = struct.unpack('<4sHI', h)
    if magic != b'AGB1' or n > 65526: raise ValueError('invalid frame')
    return kind, read_exact(s, n)

class Client:
    def __init__(self, daemon, handshake=True, open_session=False):
        if len(daemon.clients) >= MAX_PEERS: raise RuntimeError('64 peer bound exceeded')
        self.daemon = daemon
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(1.5)
        self.session = None
        self.hello_us = None
        daemon.clients.add(self)
        try:
            start = time.perf_counter_ns()
            self.sock.connect(str(daemon.directory / 'agent.sock'))
            if handshake:
                self.sock.sendall(HELLO)
                kind, body = receive(self.sock)
                self.hello_us = (time.perf_counter_ns() - start) / 1000
                if kind != 2 or len(body) not in (29, 37) or body[:2] != b'\x01\x00' or body[27] != 1 or len(body) != (37 if body[28] == 1 else 29):
                    raise ValueError(f'expected real StandaloneProcess HelloAck, got {kind}/{body.hex()}')
                if open_session:
                    self.sock.sendall(frame(4, struct.pack('<HBB', 1, 1, 2)))
                    kind, body = receive(self.sock)
                    if kind != 5 or len(body) != 19 or body[:3] != b'\x01\x01\x00':
                        raise ValueError('OpenSession failed')
                    self.session = body[3:]
        except BaseException:
            self.close()
            raise

    def resume(self):
        start = time.perf_counter_ns()
        self.sock.sendall(frame(4, struct.pack('<H', 2) + self.session))
        kind, body = receive(self.sock)
        if kind != 5 or body != b'\x01\x02\x00': raise ValueError('ResumeSession failed')
        return (time.perf_counter_ns() - start) / 1000

    def close(self):
        self.sock.close()
        self.daemon.clients.discard(self)

class Daemon:
    def __init__(self, out, name):
        self.clients = set()
        # Short pathname respects Darwin sockaddr_un length (104 bytes).
        self.directory = Path(tempfile.mkdtemp(prefix='c1192-', dir='/private/tmp'))
        os.chmod(self.directory, 0o700)
        self.log = open(out / f'{name}.stderr.log', 'wb')
        self.process = subprocess.Popen([str(BINARY), '--directory', str(self.directory), '--deadline-secs', '120'],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=self.log,
            env={'PATH': '/usr/bin:/bin', 'HOME': str(self.directory), 'TMPDIR': str(self.directory)}, start_new_session=True)
        self.closed = False
        try:
            until = time.monotonic() + 5
            while not (self.directory / 'agent.sock').exists():
                if self.process.poll() is not None: raise RuntimeError(f'daemon exited {self.process.returncode}')
                if time.monotonic() > until: raise TimeoutError('private socket not published')
                time.sleep(.01)
            time.sleep(.1)
        except BaseException:
            self.close()
            raise

    def close(self):
        if self.closed: return
        self.closed = True
        for c in list(self.clients): c.close()
        if self.process.poll() is None:
            self.process.send_signal(signal.SIGTERM)
            try: self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)
        self.log.close()
        # Remove ONLY our newly-created private directory and its known files.
        for p in self.directory.iterdir():
            if p.name not in ('agent.sock', 'agent.lock', 'agent.db', 'agent.db-shm', 'agent.db-wal'):
                raise RuntimeError(f'unexpected own-directory entry {p.name}; preserved')
            p.unlink()
        self.directory.rmdir()

def percentiles(values):
    if not values: return {'n': 0}
    v = sorted(values)
    return {'n': len(v), 'p50_us': v[math.ceil(.5*len(v))-1], 'p95_us': v[math.ceil(.95*len(v))-1], 'max_us': v[-1]}

def matrix(out, repeat, mode, n):
    name = f'matrix-r{repeat}-{mode}-{n}'
    d = Daemon(out, name)
    row = {'kind': 'matrix', 'repeat': repeat, 'mode': mode, 'n': n, 'pid': d.process.pid}
    try:
        row['baseline'] = samples(d.process.pid)
        peers = [Client(d, handshake=mode != 'stalled', open_session=mode != 'stalled') for _ in range(n)]
        row['hello_us'] = [c.hello_us for c in peers if c.hello_us is not None]
        row['resume_us'] = []
        row['loaded'] = []
        if mode == 'active':
            for _ in range(5):
                for c in peers: row['resume_us'].append(c.resume())
                row['loaded'].append(metrics(d.process.pid))
                time.sleep(.05)
        else:
            row['loaded'] = samples(d.process.pid, count=5, interval=.05)
        for c in peers: c.close()
        row['drain'] = []
        for delay in (.05, .2, .5):
            time.sleep(delay)
            row['drain'].append(metrics(d.process.pid))
        row['hello_percentiles'] = percentiles(row['hello_us'])
        row['resume_percentiles'] = percentiles(row['resume_us'])
    finally:
        d.close()
        row['daemon_exit_code'] = d.process.returncode
        row['private_directory_removed'] = not d.directory.exists()
    return row

def storm(out):
    d = Daemon(out, 'storm')
    rows = []
    try:
        trusted = Client(d, open_session=True)
        for cycle in range(1, 4):
            row = {'kind': 'storm', 'cycle': cycle, 'stalled_peers': 62, 'max_total_peers': 64,
                   'baseline_with_admitted_client': samples(d.process.pid), 'hello_us': [], 'resume_us': [], 'samples': []}
            stalled = [Client(d, handshake=False) for _ in range(62)]
            start = time.monotonic()
            # 30 new Hellos, one transient peer at once; admitted client stays live.
            for i in range(30):
                c = Client(d)
                row['hello_us'].append(c.hello_us)
                c.close()
                row['resume_us'].append(trusted.resume())
                row['samples'].append(metrics(d.process.pid))
                time.sleep(.08)
            row['duration_seconds'] = time.monotonic() - start
            # Peers remain open beyond handshake timeout; verify server EOF explicitly.
            row['stalled_peer_eof_count'] = sum(c.sock.recv(1) == b'' for c in stalled)
            for c in stalled: c.close()
            row['drain'] = []
            for delay in (.05, .2, .5):
                time.sleep(delay)
                row['drain'].append(metrics(d.process.pid))
            row['post_drain_resume_us'] = trusted.resume()
            row['hello_percentiles'] = percentiles(row['hello_us'])
            row['resume_percentiles'] = percentiles(row['resume_us'])
            rows.append(row)
    finally:
        d.close()
    return {'kind': 'storm_run', 'cycles': rows, 'daemon_exit_code': d.process.returncode,
            'private_directory_removed': not d.directory.exists()}

def provenance():
    paths = []
    for crate in ('seyal-agent-core', 'seyal-agent-protocol', 'seyal-agent-store', 'seyal-agent-backend'):
        paths.extend((SOURCE / 'crates' / crate).rglob('*.rs'))
        paths.append(SOURCE / 'crates' / crate / 'Cargo.toml')
    paths.extend([SOURCE / 'Cargo.toml', SOURCE / 'Cargo.lock'])
    return {'schema_version': 2, 'timestamp_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
            'platform': platform.platform(), 'machine': platform.machine(),
            'cpu_count': os.cpu_count(), 'python': platform.python_version(),
            'mach_timebase': {'numer': TIMEBASE.numer, 'denom': TIMEBASE.denom},
            'source_head': command(['git', 'rev-parse', 'HEAD'], SOURCE),
            'source_status': command(['git', 'status', '--short'], SOURCE),
            'source_diff_stat': command(['git', 'diff', '--stat'], SOURCE),
            'source_sha256': {str(p.relative_to(SOURCE)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(paths)},
            'binary_sha256': hashlib.sha256(BINARY.read_bytes()).hexdigest(), 'binary': str(BINARY),
            'build': 'cargo build --release --locked --offline --package seyal-agent-backend --bin seyal-agent-backend --target-dir ${EVIDENCE_ROOT}/target',
            'features': 'default; no fixture-host; real StandaloneProcessHost; empty production catalog',
            'rustc': command(['rustc', '-Vv'], SOURCE),
            'probe_shell_default_rustc': command(['rustc', '-Vv']),
            'rustc_metadata_scope': 'rustc runs in SOURCE_ROOT, matching the build cwd; probe default is recorded separately',
            'os': command(['sw_vers']),
            'hardware': {key: command(['/usr/sbin/sysctl', '-n', key]) for key in ('hw.model', 'hw.memsize', 'hw.ncpu', 'machdep.cpu.brand_string')},
            'fd_rlimit': resource.getrlimit(resource.RLIMIT_NOFILE),
            'method': {'percentiles': 'nearest-rank, all retained samples, no warmup removal',
                'resources': 'libproc PROC_PIDTASKINFO and PROC_PIDLISTFDS on own spawned PID',
                'cpu': 'raw cumulative user+system mach ticks converted by mach_timebase_info numer/denom to nanoseconds; interval CPU from deltas',
                'hello_timing': 'Python monotonic perf_counter_ns before connect through complete HelloAck receive',
                'repetitions': 3, 'population': [1,8,32,64], 'modes': ['stalled','idle','active'],
                'active': 'OpenSession RunsObserve once, then five rounds of one sequential ResumeSession per peer; sample resources each round, 50ms sleep after each round; no provider execution',
                'storm': 'three 62-stalled+one preadmitted observer+one transient Hello cycles; each over 2.4 seconds; hard max 64',
                'terminal_dimensions': None, 'terminal_latency_status': 'NOT RUN: requires real TerminalExecution workflow',
                'threshold': 'exploratory baseline only; numeric cap and acceptable regression threshold require accepted policy',
                'authentication': 'legacy public observer evidence on private temporary production instance; not first-party auth proof',
                'performance_claim': False}}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--preflight', action='store_true')
    args = parser.parse_args()
    out = ROOT / ('preflight-' if args.preflight else 'run-') / time.strftime('%Y%m%dT%H%M%SZ', time.gmtime())
    out.mkdir(parents=True)
    prov = provenance()
    (out/'metadata.json').write_text(json.dumps(prov, indent=2)+'\n')
    raw = open(out/'raw.jsonl', 'w', buffering=1)
    try:
        if args.preflight:
            row = matrix(out, 0, 'active', 1)
            raw.write(json.dumps(row)+'\n')
            print(json.dumps({'output': str(out), 'preflight': 'passed'}), flush=True)
        else:
            for repeat in range(1,4):
                for mode in ('stalled', 'idle', 'active'):
                    for n in (1,8,32,64):
                        row = matrix(out, repeat, mode, n)
                        raw.write(json.dumps(row)+'\n')
                        print(json.dumps({'repeat': repeat, 'mode': mode, 'n': n, 'done': True}), flush=True)
            raw.write(json.dumps(storm(out))+'\n')
            print(json.dumps({'output': str(out), 'status': 'passed'}), flush=True)
    except BaseException as e:
        (out/'blocker.json').write_text(json.dumps({'type': type(e).__name__, 'error': str(e)}, indent=2)+'\n')
        raise
    finally:
        raw.close()

if __name__ == '__main__':
    main()
