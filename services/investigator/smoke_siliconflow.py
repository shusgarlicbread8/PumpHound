"""Make exactly one low-token SiliconFlow connectivity call when a key exists."""

from __future__ import annotations

import os

import httpx


def main() -> int:
    api_key = os.environ.get("PUMPHOUND_SILICONFLOW_API_KEY") or os.environ.get("SILICONFLOW_API_KEY")
    if not api_key:
        print("SiliconFlow smoke skipped: API key is absent.")
        return 0

    base_url = os.environ.get(
        "PUMPHOUND_SILICONFLOW_BASE_URL",
        os.environ.get("SILICONFLOW_BASE_URL", "https://api.siliconflow.com/v1"),
    ).rstrip("/")
    model = os.environ.get(
        "PUMPHOUND_SILICONFLOW_MODEL",
        os.environ.get("SILICONFLOW_MODEL", "deepseek-ai/DeepSeek-V4-Flash"),
    )
    response = httpx.post(
        f"{base_url}/chat/completions",
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
        json={
            "model": model,
            "max_tokens": 8,
            "temperature": 0,
            "response_format": {"type": "json_object"},
            "messages": [
                {
                    "role": "user",
                    "content": 'Reply with only {"ok":true}. This is a connectivity test.',
                }
            ],
        },
        timeout=15,
    )
    response.raise_for_status()
    print(f"SiliconFlow smoke succeeded: model={model} status={response.status_code}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
