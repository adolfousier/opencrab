"""In-memory session router mirroring Telegram handler resolve logic."""

from __future__ import annotations

import uuid
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from typing import Callable

from .helpers import (
    build_dm_session_title,
    compose_auto_title,
    is_default_channel_title,
    should_refresh_label,
)

UtcNow = Callable[[], datetime]


def _utc_now() -> datetime:
    return datetime.now(timezone.utc)


@dataclass
class SessionRow:
    id: str
    title: str
    auto_title_attempted: bool = False
    updated_at: datetime = field(default_factory=_utc_now)
    archived: bool = False


class SessionStore:
    def __init__(self, now: UtcNow | None = None) -> None:
        self.rows: dict[str, SessionRow] = {}
        self.chat_sessions: dict[int, str] = {}
        self._now = now or _utc_now
        self._tick = 0

    def _bump(self, row: SessionRow) -> None:
        self._tick += 1
        row.updated_at = self._now() + timedelta(seconds=self._tick)

    def create(self, title: str) -> SessionRow:
        row = SessionRow(id=str(uuid.uuid4()), title=title)
        self.rows[row.id] = row
        self._bump(row)
        return row

    def find_by_title_suffix(self, suffix: str) -> SessionRow | None:
        matches = [
            r
            for r in self.rows.values()
            if not r.archived and r.title.endswith(suffix)
        ]
        if not matches:
            return None
        return max(matches, key=lambda r: r.updated_at)

    def touch(self, session_id: str) -> None:
        row = self.rows[session_id]
        self._bump(row)

    def set_title(self, session_id: str, title: str) -> None:
        row = self.rows[session_id]
        row.title = title
        self._bump(row)

    def mark_auto_title_attempted(self, session_id: str) -> None:
        self.rows[session_id].auto_title_attempted = True

    def reset_auto_title_attempted(self, session_id: str) -> None:
        self.rows[session_id].auto_title_attempted = False


def _apply_label_drift(
    store: SessionStore,
    row: SessionRow,
    template: str,
    *,
    safe: bool,
) -> None:
    if safe:
        if should_refresh_label(row.title, template):
            store.set_title(row.id, template)
    else:
        # Production bug: any mismatch resets to template
        if row.title != template:
            store.set_title(row.id, template)


def resolve_suffix_only(
    store: SessionStore,
    chat_id: int,
    user_name: str,
    user_id: int,
    *,
    is_dm: bool = True,
    chat_title: str = "",
) -> SessionRow:
    template = (
        build_dm_session_title(user_name, user_id, chat_id)
        if is_dm
        else build_group_session_title(chat_title or "Group", chat_id)
    )
    suffix = f"[chat:{chat_id}]"
    existing = store.find_by_title_suffix(suffix)
    if existing:
        _apply_label_drift(store, existing, template, safe=False)
        return store.rows[existing.id]
    return store.create(template)


def resolve_with_chat_map(
    store: SessionStore,
    chat_id: int,
    user_name: str,
    user_id: int,
    *,
    is_dm: bool = True,
    chat_title: str = "",
) -> SessionRow:
    template = (
        build_dm_session_title(user_name, user_id, chat_id)
        if is_dm
        else build_group_session_title(chat_title or "Group", chat_id)
    )
    suffix = f"[chat:{chat_id}]"

    bound = store.chat_sessions.get(chat_id)
    if bound and bound in store.rows and not store.rows[bound].archived:
        row = store.rows[bound]
        _apply_label_drift(store, row, template, safe=True)
        return row

    existing = store.find_by_title_suffix(suffix)
    if existing:
        _apply_label_drift(store, existing, template, safe=True)
        store.chat_sessions[chat_id] = existing.id
        return store.rows[existing.id]

    row = store.create(template)
    store.chat_sessions[chat_id] = row.id
    return row


def maybe_run_auto_title(
    store: SessionStore,
    session_id: str,
    user_message: str,
    llm_title: str,
    *,
    llm_failed: bool = False,
) -> None:
    row = store.rows[session_id]
    if llm_failed:
        store.reset_auto_title_attempted(session_id)
        return
    if row.auto_title_attempted:
        return
    if user_message.strip() and (
        not row.title or is_default_channel_title(row.title)
    ):
        store.mark_auto_title_attempted(session_id)
        new_title = compose_auto_title(row.title, llm_title)
        if new_title:
            store.set_title(session_id, new_title)
