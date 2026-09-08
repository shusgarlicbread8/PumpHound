from typing import Any

__all__ = ["ModelRegistry"]


def __getattr__(name: str) -> Any:
    if name == "ModelRegistry":
        from pumphound_ml.registry.store import ModelRegistry

        return ModelRegistry
    raise AttributeError(name)
