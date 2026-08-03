from __future__ import annotations
import logging
from config import get_settings
from http_client import build_session
from security.errors import GENERIC_USER_ERROR
from security.rate_limit import FixedWindowRateLimiter, RateLimitExceeded
from security.subprocess_guard import CommandExecutionError, CommandNotAllowedError, run_safe_command
logger = logging.getLogger(__name__)
settings = get_settings()
rate_limiter = FixedWindowRateLimiter(max_requests=settings.rate_limit_max_requests, window_seconds=settings.rate_limit_window_seconds, redis_url=settings.redis_url)
http_session = build_session(auth_token=settings.auth_token)
def _process_command(command: str) -> str:
    output = run_safe_command(command, allowed_commands=settings.allowed_commands, timeout=10)
    if not output.strip(): return "Command completed with no output."
    return output
def handle_message(user_id: str, message: str) -> str:
    try:
        rate_limiter.check(user_id)
        text = message.strip()
        if not text: return "Please send a non-empty message."
        if text.lower().startswith("/run "):
            command = text[5:].strip()
            return _process_command(command)
        return "Message received."
    except RateLimitExceeded as exc: return f"Slow down. Try again in {exc.retry_after_seconds} seconds."
    except CommandNotAllowedError: return "That command is not allowed."
    except CommandExecutionError: return "The command could not be executed safely."
    except Exception:
        logger.exception("Unhandled error in handle_message")
        return GENERIC_USER_ERROR
