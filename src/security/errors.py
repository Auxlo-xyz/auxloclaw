from __future__ import annotations
import functools, logging, uuid
from typing import Callable, TypeVar
logger = logging.getLogger(__name__)
T = TypeVar("T")
GENERIC_USER_ERROR = "I encountered an error while processing your request."
def safe_user_response(func: Callable[..., T] | None = None, *, user_message: str = GENERIC_USER_ERROR) -> Callable[..., T]:
    def decorator(f: Callable[..., T]) -> Callable[..., T]:
        @functools.wraps(f)
        def wrapper(*args, **kwargs) -> T:
            try: return f(*args, **kwargs)
            except Exception:
                error_id = uuid.uuid4().hex[:8]
                logger.exception("Unhandled exception in %s error_id=%s", f.__name__, error_id)
                return user_message
        return wrapper
    if func is None: return decorator
    return decorator(func)
