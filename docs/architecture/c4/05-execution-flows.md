# C4 Dynamic Views — Execution Flows

These views explain important runtime interactions. They are descriptive projections of accepted ADR/spec behavior.

## Output path

```mermaid
sequenceDiagram
    participant App as Shell / application
    participant Exec as seyal-exec / PTY
    participant Term as seyal-terminal
    participant Runtime as seyal-runtime
    participant Client as seyal-client
    participant Render as seyal-render
    participant Host as thin macOS host / Metal

    App->>Exec: terminal bytes
    Exec->>Term: byte stream
    Term->>Term: mutate authoritative TerminalState + damage
    Term-->>Runtime: derived state / damage
    Runtime-->>Client: versioned projection
    Client-->>Render: derived render model
    Render-->>Host: prepared render data
    Host-->>Host: encode / present pixels
```

No stage after `seyal-terminal` becomes a second terminal authority.

## Input path

```mermaid
sequenceDiagram
    participant User as Keyboard / IME / mouse
    participant Host as thin native host
    participant Client as Rust product/client
    participant Runtime as seyal-runtime
    participant Exec as seyal-exec / PTY
    participant App as Shell / application

    User->>Host: native event / preedit
    Host->>Client: typed event or committed text
    Client->>Client: presentation + focus policy
    Client->>Runtime: bounded input/control request
    Runtime->>Runtime: terminal-mode-aware routing/encoding
    Runtime->>Exec: PTY input
    Exec->>App: terminal input
```

Ephemeral native IME preedit state stays native; committed input enters the normal Rust/runtime path.

## Flow command path

```mermaid
flowchart LR
    A[Flow composer] -->|submit| B[Rust composer revision / correlation]
    B --> C[Runtime input]
    C --> D[PTY / shell]
    D --> E[seyal-terminal TerminalState / history]
    E --> F[trusted command boundaries + history anchors]
    F --> G[Block projection]
```

A Block represents real terminal execution. It does not create another PTY, VT engine, process or transcript authority.

## Flow ↔ Raw/TUI transition

```mermaid
flowchart TB
    A[Current presentation] --> B[Freeze / revoke stale input, focus, IME and mouse routes]
    B --> C[Validate current execution, attachment and generation]
    C --> D{Activate exactly one presentation}
    D --> E[Flow]
    D --> F[Raw]
    D --> G[TUI]
```

There is no simultaneous hidden raw-terminal input surface underneath Flow.

## GUI close / reopen

```mermaid
sequenceDiagram
    participant GUI as Seyal GUI
    participant Runtime as per-user Runtime
    participant Exec as TerminalExecution
    participant PTY as PTY + child
    participant Term as seyal-terminal

    GUI->>Runtime: attached
    GUI--xGUI: close / crash
    Note over Runtime,Term: Runtime, execution, PTY/child and canonical terminal state continue
    GUI->>Runtime: later reconnect
    Runtime->>Runtime: validate execution + fresh attachment
    Runtime-->>GUI: current projection
    GUI->>GUI: rebuild disposable client/render state
```

Detach is not terminate.

## Explicit termination

Explicit execution termination is a Runtime/execution lifecycle operation. It must respect ownership, other attached controllers where applicable, child reaping/final drain and resource cleanup. GUI destruction alone must not masquerade as this operation.

## Persistence distinction

```text
GUI survival boundary        Runtime can outlive GUI
Runtime crash boundary       separate problem
live PTY restoration         cannot be recreated by journal/history alone
reboot recovery              separate problem
history/workspace durability separate persistence problem
```

Do not claim that persisted terminal text or event journals restore a live PTY/process.

## Hot-path isolation

Agents, semantic extraction, persistence, cloud and telemetry may be additive systems, but none may become a synchronous dependency of PTY → VT → terminal-state → render progress.
