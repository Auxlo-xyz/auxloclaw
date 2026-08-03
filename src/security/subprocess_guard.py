from __future__ import annotations
import logging, os, shlex, shutil, subprocess
from pathlib import Path
from typing import Iterable, List, Sequence, Union
logger = logging.getLogger(__name__)
class CommandNotAllowedError(RuntimeError): pass
class CommandExecutionError(RuntimeError): pass
def _normalize_command(command: Union[str, Sequence[str]]) -> List[str]:
    if isinstance(command, str): parts = shlex.split(command)
    else: parts = list(command)
    if not parts: raise CommandNotAllowedError("Command is empty")
    return [str(part) for part in parts]
def _resolve_executable(executable: str) -> Path:
    if os.sep in executable or (os.altsep and os.altsep in executable):
        path = Path(executable).expanduser()
        if not path.is_absolute(): raise CommandNotAllowedError("Relative executable paths are not allowed")
    else:
        located = shutil.which(executable)
        if not located: raise CommandExecutionError(f"Executable not found: {executable}")
        path = Path(located)
    try: resolved = path.resolve(strict=True)
    except OSError as exc: raise CommandExecutionError(f"Unable to resolve executable: {executable}") from exc
    if not resolved.is_file(): raise CommandNotAllowedError(f"Executable is not a file: {resolved}")
    return resolved
def _executable_is_allowed(resolved_executable: Path, allowed_commands: Iterable[str]) -> bool:
    allowed = list(allowed_commands)
    allowed_absolute_paths = set()
    allowed_names = set()
    for item in allowed:
        item = item.strip().lower()
        if not item: continue
        if os.sep in item or (os.altsep and os.altsep in item):
            try:
                resolved_allowed = Path(item).expanduser().resolve(strict=True)
                allowed_absolute_paths.add(str(resolved_allowed))
            except OSError: pass
        else: allowed_names.add(Path(item).name.lower())
    if str(resolved_executable) in allowed_absolute_paths: return True
    if resolved_executable.name.lower() in allowed_names: return True
    return False
def run_safe_command(command: Union[str, Sequence[str]], *, allowed_commands: Iterable[str], timeout: int = 30, cwd: Union[str, Path, None] = None, max_output_bytes: int = 100000) -> str:
    parts = _normalize_command(command)
    if len(parts) > 32: raise CommandNotAllowedError("Command has too many arguments")
    for part in parts:
        if len(part) > 4096: raise CommandNotAllowedError("Command argument is too long")
    resolved_executable = _resolve_executable(parts[0])
    if not _executable_is_allowed(resolved_executable, allowed_commands):
        raise CommandNotAllowedError(f"Executable is not allowed: {resolved_executable.name}")
    safe_command = [str(resolved_executable)] + parts[1:]
    try:
        result = subprocess.run(safe_command, shell=False, capture_output=True, timeout=timeout, cwd=cwd, check=False)
    except subprocess.TimeoutExpired as exc: raise CommandExecutionError("Command timed out") from exc
    except FileNotFoundError as exc: raise CommandExecutionError("Executable not found") from exc
    except PermissionError as exc: raise CommandExecutionError("Permission denied while running command") from exc
    except Exception as exc: raise CommandExecutionError("Command execution failed") from exc
    stdout = result.stdout[:max_output_bytes]
    stderr = result.stderr[:max_output_bytes]
    if result.returncode != 0:
        logger.warning("Command failed. executable=%s returncode=%s stderr=%s", resolved_executable, result.returncode, stderr.decode(errors="replace")[:1000])
        raise CommandExecutionError(f"Command exited with code {result.returncode}")
    return stdout.decode(errors="replace")
