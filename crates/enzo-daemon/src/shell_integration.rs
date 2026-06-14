//! OSC-133 shell integration injected at PTY spawn.
//!
//! When the user's shell is supported (bash today), we launch it with a
//! generated rc file that sources the real init files and then wraps the prompt
//! with [OSC 133] semantic marks (`A` prompt-start, `B` command-start, `C`
//! pre-exec, `D;exit` command-end). The terminal client uses these to turn the
//! raw byte stream into addressable command blocks. Anything unsupported, or any
//! setup failure, falls back to a plain interactive shell — the terminal still
//! works, it just has no semantic blocks.
//!
//! [OSC 133]: https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md

use std::io::Write;
use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;

/// bash rc: source the real init files, then add OSC-133 prompt marks.
/// `\033` (ESC) and `\007` (BEL) are written literally for `printf`/PS1 to
/// interpret.
const BASH_RC: &str = r##"# Enzo OSC-133 shell integration (generated)
[ -r /etc/bash.bashrc ] && . /etc/bash.bashrc
[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"
__enzo_report_exit() { printf '\033]133;D;%s\007' "$?"; }
case "${PROMPT_COMMAND:-}" in
  *__enzo_report_exit*) ;;
  *) PROMPT_COMMAND="__enzo_report_exit${PROMPT_COMMAND:+; $PROMPT_COMMAND}" ;;
esac
PS1='\[\033]133;A\007\]'"$PS1"'\[\033]133;B\007\]'
PS0='\033]133;C\007'"${PS0:-}"
"##;

/// Build a [`CommandBuilder`] for `shell`, injecting OSC-133 integration when
/// supported, else a plain shell.
#[must_use]
pub fn command_for(shell: &str) -> CommandBuilder {
    let name = Path::new(shell)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if name == "bash"
        && let Some(rc) = write_rc("enzo-bash-integration.bash", BASH_RC)
    {
        // No `-i`: the PTY already makes bash interactive (so `--rcfile` is
        // honoured), and `-i` enables job control that deadlocks in headless
        // test PTYs without a controlling terminal.
        let mut cmd = CommandBuilder::new(shell);
        cmd.arg("--rcfile");
        cmd.arg(&rc);
        return cmd;
    }
    CommandBuilder::new(shell)
}

/// Write `content` to a stable per-runtime path, returning it on success.
fn write_rc(name: &str, content: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join(name);
    let mut file = std::fs::File::create(&path).ok()?;
    file.write_all(content.as_bytes()).ok()?;
    Some(path)
}
