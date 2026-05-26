"""Event API for Telegram session simulation."""

from __future__ import annotations

from .helpers import build_dm_session_title
from .router import (
    SessionStore,
    maybe_run_auto_title,
    resolve_suffix_only,
    resolve_with_chat_map,
)


class TelegramSessionSim:
    def __init__(self, *, use_fixed_resolver: bool = False) -> None:
        self.store = SessionStore()
        self.use_fixed_resolver = use_fixed_resolver
        self._resolve = (
            resolve_with_chat_map if use_fixed_resolver else resolve_suffix_only
        )

    def on_message(
        self,
        chat_id: int,
        user_name: str,
        user_id: int,
        text: str = "hello",
        *,
        is_dm: bool = True,
        chat_title: str = "",
    ) -> str:
        row = self._resolve(
            self.store, chat_id, user_name, user_id, is_dm=is_dm, chat_title=chat_title
        )
        llm_title = " ".join(text.split()[:5]) or "New topic"
        maybe_run_auto_title(self.store, row.id, text, llm_title)
        return row.id

    def on_new(
        self,
        chat_id: int,
        user_name: str,
        user_id: int,
        *,
        is_owner: bool = True,
    ) -> str:
        row = self.store.create(build_dm_session_title(user_name, user_id, chat_id))
        if self.use_fixed_resolver:
            self.store.chat_sessions[chat_id] = row.id
        return row.id

    def on_sessions_switch(self, chat_id: int, session_id: str) -> None:
        self.store.touch(session_id)
        self.store.chat_sessions[chat_id] = session_id

    def on_auto_title_complete(self, session_id: str, llm_title: str) -> None:
        maybe_run_auto_title(self.store, session_id, "seed", llm_title)
