from __future__ import annotations
from typing import Any, Dict, Optional
import requests
from requests.adapters import HTTPAdapter
from urllib3.util.retry import Retry
class InsecureRequestError(RuntimeError): pass
def build_session(auth_token: Optional[str] = None, user_agent: str = "auxloclaw-hardened/1.0") -> requests.Session:
    session = requests.Session()
    session.verify = True
    session.headers.update({"User-Agent": user_agent, "Accept": "application/json"})
    if auth_token: session.headers.update({"Authorization": f"Bearer {auth_token}"})
    retry = Retry(total=3, connect=2, read=2, status=3, backoff_factor=0.3, status_forcelist=(429, 500, 502, 503, 504), allowed_methods=frozenset({"GET", "POST"}), raise_on_status=False)
    adapter = HTTPAdapter(max_retries=retry)
    session.mount("https://", adapter)
    return session
def request_json(session: requests.Session, method: str, url: str, *, timeout: int = 10, **kwargs: Any) -> Dict[str, Any]:
    if not url.lower().startswith("https://"): raise InsecureRequestError("Only HTTPS URLs are allowed")
    response = session.request(method, url, timeout=timeout, **kwargs)
    response.raise_for_status()
    return response.json()
