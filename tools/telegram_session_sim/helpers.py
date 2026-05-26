"""Pure helpers ported from opencrabs src/brain/agent/service/types.rs."""

from __future__ import annotations

CHANNEL_PREFIXES = (
    "Telegram: ",
    "Discord: ",
    "Slack: ",
    "WhatsApp: ",
    "Trello: ",
)


def clean_auto_title(raw: str) -> str:
    trimmed = raw.strip().strip('"').strip("'")
    if not trimmed:
        return ""
    if len(trimmed) > 60:
        return trimmed[:60]
    return trimmed


def is_default_channel_title(title: str) -> bool:
    if title == "New Chat":
        return True
    if title.startswith("Telegram: "):
        rest = title[len("Telegram: ") :]
        return rest.startswith("DM ") and "(" in rest and ")" in rest
    if title.startswith("Discord: "):
        return title[len("Discord: ") :].startswith("#")
    if title.startswith("Slack: "):
        return title[len("Slack: ") :].startswith("#")
    return False


def extract_channel_prefix(title: str) -> str:
    for prefix in CHANNEL_PREFIXES:
        if title.startswith(prefix):
            return prefix
    return ""


def extract_chat_id_suffix(title: str) -> str:
    pos = title.rfind("[chat:")
    if pos == -1:
        return ""
    suffix = title[pos:]
    if suffix.endswith("]"):
        return suffix
    return ""


def compose_auto_title(old_title: str, llm_title: str) -> str:
    clean = clean_auto_title(llm_title)
    if not clean:
        return old_title
    prefix = extract_channel_prefix(old_title)
    chat_suffix = extract_chat_id_suffix(old_title)
    if not prefix:
        return f"{clean} {chat_suffix}".strip() if chat_suffix else clean
    if not chat_suffix:
        return f"{prefix}{clean}"
    return f"{prefix}{clean} {chat_suffix}"


def build_dm_session_title(user_name: str, user_id: int, chat_id: int) -> str:
    suffix = f"[chat:{chat_id}]"
    return f"Telegram: DM {user_name} ({user_id}) {suffix}"


def build_group_session_title(chat_title: str, chat_id: int) -> str:
    return f"Telegram: {chat_title} [chat:{chat_id}]"


def is_telegram_group_session_title(title: str) -> bool:
    if not title.startswith("Telegram: "):
        return False
    rest = title[len("Telegram: ") :]
    if rest.startswith("DM "):
        return False
    return "[chat:" in title


def telegram_middle_label(title: str) -> str:
    """Label between 'Telegram: ' and ' [chat:N]'."""
    if not title.startswith("Telegram: "):
        return title
    body = title[len("Telegram: ") :]
    suffix = extract_chat_id_suffix(title)
    if suffix and body.endswith(suffix):
        body = body[: -len(suffix)].rstrip()
    return body


def should_refresh_label(stored: str, template: str) -> bool:
    """Fixed label-drift policy (issue #121 + group rename stability)."""
    if stored == template:
        return False
    if is_default_channel_title(stored):
        return is_default_channel_title(template) and stored != template
    if is_telegram_group_session_title(stored) and is_telegram_group_session_title(
        template
    ):
        return telegram_middle_label(stored) != telegram_middle_label(template)
    return False
