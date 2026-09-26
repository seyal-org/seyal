# Seyal trusted shell integration bootstrap for zsh.
# Authority: docs/architecture/ADR-009-COMMAND-BLOCKS-COMPOSER-AND-TUI.md,
# "2026-09-16 accepted amendment", mechanisms 1 and 3.
#
# Runtime points ZDOTDIR at the directory holding this file for exactly one
# TerminalExecution. Everything below runs before any user-owned file, in
# the order the ADR makes normative. Per-command hooks use builtins only.

# 1. Consume the per-execution secret from the inherited descriptor, then
#    close it and forget its number. The secret lives only in a non-exported
#    shell parameter; children never see it.
typeset -g _seyal_nonce=
if [[ -n "${SEYAL_NONCE_FD-}" && "${SEYAL_NONCE_FD}" == <-> ]]; then
  IFS= builtin read -r _seyal_nonce <&"${SEYAL_NONCE_FD}" 2>/dev/null
  # Silence only the close. A bare `exec` makes its redirections permanent,
  # so `2>/dev/null` on the exec itself would discard the shell's stderr
  # for its whole lifetime (#1046).
  { builtin exec {SEYAL_NONCE_FD}<&- } 2>/dev/null
fi
builtin unset SEYAL_NONCE_FD
if [[ ${#_seyal_nonce} -ne 32 || "${_seyal_nonce}" == *[^0-9a-f]* ]]; then
  _seyal_nonce=
fi

# 2. Restore ZDOTDIR to the user's value (or unset it) before any user file
#    is sourced, so every remaining startup file and every child process
#    resolves the user's real configuration. The bundled directory is never
#    referenced again.
if [[ -n "${SEYAL_USER_ZDOTDIR+set}" ]]; then
  ZDOTDIR="${SEYAL_USER_ZDOTDIR}"
else
  builtin unset ZDOTDIR
fi
builtin unset SEYAL_USER_ZDOTDIR

# 3. Source the user's own .zshenv.
if [[ -r "${ZDOTDIR:-$HOME}/.zshenv" ]]; then
  builtin source "${ZDOTDIR:-$HOME}/.zshenv"
fi

# 4. Interactive shells only, with a secret and no prior install: defer the
#    real hook installation to the first precmd, which zsh runs only after
#    .zprofile, .zshrc and .zlogin have completed.
if [[ -o interactive && -n "${_seyal_nonce}" && -z "${_seyal_integration_installed-}" ]]; then
  _seyal_preexec() {
    builtin emulate -L zsh
    builtin printf '\033]133;C;%s\007' "${_seyal_nonce}"
    typeset -g _seyal_command_open=1
  }
  _seyal_precmd() {
    local _seyal_status=$?
    builtin emulate -L zsh
    if [[ -n "${_seyal_command_open-}" ]]; then
      builtin printf '\033]133;D;%s;%s\007' "${_seyal_nonce}" "${_seyal_status}"
      _seyal_command_open=
    fi
    builtin printf '\033]133;A;%s\007' "${_seyal_nonce}"
  }
  _seyal_deferred_init() {
    builtin emulate -L zsh
    precmd_functions=(${precmd_functions:#_seyal_deferred_init})
    typeset -g _seyal_integration_installed=1
    typeset -g _seyal_command_open=
    typeset -ga preexec_functions precmd_functions
    preexec_functions+=(_seyal_preexec)
    precmd_functions+=(_seyal_precmd)
    _seyal_precmd
  }
  typeset -ga precmd_functions
  precmd_functions+=(_seyal_deferred_init)
fi
