"""Task storage with pluggable backends."""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Iterable, Optional

from .model import Task
from . import events


DEFAULT_PATH = "tasks.json"


@dataclass
class Record:
    """One stored row."""
    id: int
    title: str
    tags: list[str] = field(default_factory=list)


class Backend:
    """Abstract storage backend."""

    def load(self) -> list[Record]:
        raise NotImplementedError

    def save(self, records: Iterable[Record]) -> None:
        raise NotImplementedError


class JsonBackend(Backend):
    """Stores records as JSON on disk."""

    def __init__(self, path: str = DEFAULT_PATH, *, pretty: bool = False):
        self.path = path
        self.pretty = pretty
        self._cache: Optional[list[Record]] = None

    def load(self) -> list[Record]:
        """Read every record (cached)."""
        if self._cache is None:
            with open(self.path) as f:
                self._cache = [Record(**r) for r in json.load(f)]
        return self._cache

    def save(self, records):
        data = [r.__dict__ for r in records]
        with open(self.path, "w") as f:
            json.dump(data, f, indent=2 if self.pretty else None)
        events.emit("saved", len(data))

    @property
    def size(self) -> int:
        return len(self.load())

    @staticmethod
    def default() -> "JsonBackend":
        return JsonBackend()


class Store:
    def __init__(self, backend: Backend):
        self.backend = backend

    def add(self, title: str, *tags: str) -> Record:
        """Add a task and persist it."""
        records = self.backend.load()
        rec = Record(id=len(records) + 1, title=title, tags=list(tags))
        records.append(rec)
        self.backend.save(records)
        return rec

    async def sync(self, remote, retries: int = 3) -> bool:
        for _ in range(retries):
            if await remote.push(self.backend.load()):
                return True
        return False

    def find(self, pred=lambda r: True):
        def matches(r):
            return pred(r) and r.title
        return [r for r in self.backend.load() if matches(r)]


def open_store(path: str = DEFAULT_PATH) -> Store:
    """Store backed by a JSON file."""
    return Store(JsonBackend(path))


def to_task(rec: Record) -> Task:
    return Task(rec.id, rec.title)


if __name__ == "__main__":
    print(open_store().add("demo"))
