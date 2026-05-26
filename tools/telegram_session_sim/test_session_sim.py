"""TDD contract tests for Telegram session resolve (issue #121).

Correct-behavior tests are parametrized over the resolver:

  production  — mirrors current handler (suffix + naive label drift)
  fixed       — chat_sessions map + should_refresh_label

Run:
  pytest -q                    # expect FAILURES on [production] (red)
  pytest -q -k fixed           # green when developing the fix
  pytest -q -k production      # red until production matches fixed
"""

from __future__ import annotations

import pytest

from .helpers import (
    build_dm_session_title,
    compose_auto_title,
    is_default_channel_title,
    should_refresh_label,
)
from .router import SessionStore, maybe_run_auto_title
from .sim import TelegramSessionSim

RESOLVERS = (
    pytest.param(False, id="production"),
    pytest.param(True, id="fixed"),
)


# ── Contract tests (correct behavior) ─────────────────────────────────────


@pytest.mark.parametrize("use_fixed_resolver", RESOLVERS)
def test_auto_title_survives_second_message(use_fixed_resolver: bool):
    """After auto-title, message 2 must not revert to the default DM template."""
    sim = TelegramSessionSim(use_fixed_resolver=use_fixed_resolver)
    chat_id = 133526395
    sid = sim.on_message(chat_id, "Alexey", 133526395, "fix deploy pipeline")
    title_after_first = sim.store.rows[sid].title
    assert not is_default_channel_title(title_after_first), "auto-title should run on msg 1"

    sim.on_message(chat_id, "Alexey", 133526395, "second message")
    title_after_second = sim.store.rows[sid].title

    assert not is_default_channel_title(title_after_second), (
        "label drift must not clobber auto-titled session (#121)"
    )
    assert title_after_second == title_after_first


@pytest.mark.parametrize("use_fixed_resolver", RESOLVERS)
def test_switch_survives_updated_at_race(use_fixed_resolver: bool):
    """After /sessions switch, a background touch on another row must not steal routing."""
    store = SessionStore()
    chat_id = 42
    a = store.create(build_dm_session_title("A", 1, chat_id))
    b = store.create(build_dm_session_title("A", 1, chat_id))
    store.touch(a.id)

    sim = TelegramSessionSim(use_fixed_resolver=use_fixed_resolver)
    sim.store = store
    sim.on_sessions_switch(chat_id, a.id)

    # Simulate RSI / another session row getting a newer updated_at
    store.touch(b.id)

    sid = sim.on_message(chat_id, "A", 1, "ping")
    assert sid == a.id, "message must route to the session user switched to"


@pytest.mark.parametrize("use_fixed_resolver", RESOLVERS)
def test_new_then_switch_back_uses_older_session(use_fixed_resolver: bool):
    """/new creates B; switch to A; next message must hit A."""
    sim = TelegramSessionSim(use_fixed_resolver=use_fixed_resolver)
    chat_id = 200
    sid_a = sim.on_message(chat_id, "U", 1, "first topic")
    sid_b = sim.on_new(chat_id, "U", 1, is_owner=True)
    assert sid_a != sid_b

    sim.on_sessions_switch(chat_id, sid_a)
    sid_msg = sim.on_message(chat_id, "U", 1, "back to first")
    assert sid_msg == sid_a


# ── Unit tests (pure helpers, always green) ───────────────────────────────


def test_auto_title_retries_after_llm_failure():
    store = SessionStore()
    row = store.create(build_dm_session_title("U", 1, 99))
    maybe_run_auto_title(store, row.id, "hello", "", llm_failed=True)
    assert not store.rows[row.id].auto_title_attempted
    maybe_run_auto_title(store, row.id, "hello", "Deploy fix")
    assert not is_default_channel_title(store.rows[row.id].title)


def test_suffix_lookup_picks_most_recent_without_switch():
    store = SessionStore()
    chat_id = 7
    older = store.create(build_dm_session_title("U", 1, chat_id))
    newer = store.create(build_dm_session_title("U", 1, chat_id))
    store.touch(older.id)
    store.touch(newer.id)
    hit = store.find_by_title_suffix(f"[chat:{chat_id}]")
    assert hit is not None
    assert hit.id == newer.id


def test_should_refresh_label_group_rename():
    old = "Telegram: Old Group [chat:-1]"
    new = "Telegram: New Group [chat:-1]"
    assert should_refresh_label(old, new) is True


def test_should_refresh_label_skips_auto_titled_dm():
    auto = "Telegram: Fix deploy [chat:133526395]"
    template = build_dm_session_title("Alexey", 133526395, 133526395)
    assert should_refresh_label(auto, template) is False


def test_compose_auto_title_preserves_suffix():
    old = build_dm_session_title("A", 1, 42)
    out = compose_auto_title(old, '"Deploy fix"')
    assert out.endswith("[chat:42]")
    assert "Deploy fix" in out


def test_default_dm_title_is_detected():
    t = build_dm_session_title("Alice", 1, 99)
    assert is_default_channel_title(t)
