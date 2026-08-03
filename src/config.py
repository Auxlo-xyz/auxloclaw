from __future__ import annotations
import os
from dataclasses import dataclass
from functools import lru_cache
from typing import FrozenSet, Optional
from dotenv import load_dotenv
load_dotenv()
class ConfigError(RuntimeError): pass
def _require_env(name: str) -> str:
    value = os.getenv(name, "").strip()
    if not value: raise ConfigError(f"Missing required environment variable: {name}")
    return value
def _env_int(name: str, default: int) -> int:
    raw = os.getenv(name, "").strip()
    if not raw: return default
    try: return int(raw)
    except ValueError as exc: raise ConfigError(f"Environment variable {name} must be an integer") from exc
@dataclass(frozen=True)
class Settings:
    openai_api_key: str
    auth_token: str
    redis_url: Optional[str]
    rate_limit_max_requests: int
    rate_limit_window_seconds: int
    allowed_commands: FrozenSet[str]
    http_timeout_seconds: int
@lru_cache(maxsize=1)
def get_settings() -> Settings:
    allowed_commands = frozenset(item.strip().lower() for item in os.getenv("ALLOWED_COMMANDS", "").split(",") if item.strip())
    return Settings(
        openai_api_key=_require_env("OPENAI_API_KEY"),
        auth_token=_require_env("AUXLOCLAW_AUTH_TOKEN"),
        redis_url=os.getenv("REDIS_URL", "").strip() or None,
        rate_limit_max_requests=_env_int("RATE_LIMIT_MAX_REQUESTS", 10),
        rate_limit_window_seconds=_env_int("RATE_LIMIT_WINDOW_SECONDS", 60),
        allowed_commands=allowed_commands,
        http_timeout_seconds=_env_int("HTTP_TIMEOUT_SECONDS", 10),
    )
