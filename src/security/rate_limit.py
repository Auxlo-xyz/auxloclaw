from __future__ import annotations
import logging, time
from abc import ABC, abstractmethod
from threading import Lock
from typing import Dict, Optional, Tuple
logger = logging.getLogger(__name__)
class RateLimitExceeded(RuntimeError):
    def __init__(self, retry_after_seconds: int):
        self.retry_after_seconds = retry_after_seconds
        super().__init__(f"Rate limit exceeded. Retry after {retry_after_seconds} seconds.")
class RateLimiter(ABC):
    @abstractmethod
    def check(self, key: str) -> None: pass
class FixedWindowRateLimiter(RateLimiter):
    def __init__(self, max_requests: int, window_seconds: int, redis_url: Optional[str] = None):
        if max_requests <= 0: raise ValueError("max_requests must be > 0")
        if window_seconds <= 0: raise ValueError("window_seconds must be > 0")
        self.max_requests = max_requests
        self.window_seconds = window_seconds
        self._redis = None
        self._memory: Dict[str, Tuple[int, int]] = {}
        self._lock = Lock()
        if redis_url:
            try:
                import redis
                self._redis = redis.Redis.from_url(redis_url, decode_responses=True, socket_timeout=1.0, socket_connect_timeout=1.0)
                self._redis.ping()
            except Exception:
                logger.warning("Redis unavailable. Falling back to memory.")
                self._redis = None
    def check(self, key: str) -> None:
        if self._redis is not None:
            try:
                self._check_redis(key)
                return
            except Exception:
                logger.exception("Redis rate-limit check failed.")
                self._redis = None
        self._check_memory(key)
    def _check_redis(self, key: str) -> None:
        redis_key = f"auxloclaw:rate_limit:{key}"
        current = self._redis.incr(redis_key, 1)
        if current == 1: self._redis.expire(redis_key, self.window_seconds)
        if current > self.max_requests:
            ttl = self._redis.ttl(redis_key)
            if ttl is None or ttl < 0: ttl = self.window_seconds
            raise RateLimitExceeded(int(ttl))
    def _check_memory(self, key: str) -> None:
        now = int(time.time())
        window_start = now - (now % self.window_seconds)
        with self._lock:
            if len(self._memory) > 10000:
                self._memory = {k: v for k, v in self._memory.items() if v[0] == window_start}
            entry = self._memory.get(key)
            if entry is None or entry[0] != window_start:
                self._memory[key] = (window_start, 1)
                return
            current_count = entry[1]
            if current_count >= self.max_requests:
                retry_after = window_start + self.window_seconds - now
                raise RateLimitExceeded(max(1, retry_after))
            self._memory[key] = (window_start, current_count + 1)
