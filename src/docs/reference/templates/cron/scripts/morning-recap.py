#!/usr/bin/env python3
"""Morning Recap template for OpenCrabs.

Generates a daily summary of:
- GitHub stats (stars, forks, clones, views)
- Active sessions (last 24h) with last user message
- Cron job health (pass/fail counts)

Delivers via Telegram's sendRichMessage API with HTML fallback.

SETUP:
1. Copy this script to ~/.opencrabs/scripts/morning-recap.py
2. Edit TELEGRAM_CHAT_ID below (or set via environment variable)
3. Ensure ~/.opencrabs/github_stats.json exists (populated by GitHub stats cron)
4. Create cron job:
   Name: Morning Recap
   Schedule: 0 8 * * * (8am daily)
   Timezone: Your timezone (e.g., America/New_York)
   Prompt: Run this command: python3 ~/.opencrabs/scripts/morning-recap.py

CUSTOMIZATION:
- Change TELEGRAM_CHAT_ID to your chat ID
- Modify format_recap() to add/remove sections
- Adjust time window (currently 24h) in get_recent_sessions() and get_cron_summary()
"""

import sqlite3
import json
import os
import re
import urllib.request
import urllib.error
from datetime import datetime, timedelta, timezone

# ============================================================================
# CONFIGURATION — Edit these values
# ============================================================================

DB_PATH = os.path.expanduser("~/.opencrabs/opencrabs.db")
KEYS_PATH = os.path.expanduser("~/.opencrabs/keys.toml")
STATS_PATH = os.path.expanduser("~/.opencrabs/github_stats.json")

# Your Telegram chat ID (get it from @userinfobot or check your bot's logs)
TELEGRAM_CHAT_ID = int(os.environ.get("TELEGRAM_CHAT_ID", "YOUR_CHAT_ID_HERE"))

# ============================================================================
# TELEGRAM DELIVERY
# ============================================================================

def get_bot_token():
    """Extract Telegram bot token from keys.toml."""
    in_section = False
    with open(KEYS_PATH, "r") as f:
        for line in f:
            stripped = line.strip()
            if stripped == "[channels.telegram]":
                in_section = True
                continue
            if in_section and stripped.startswith("["):
                break
            if in_section:
                m = re.match(r'^token\s*=\s*"(.+)"', stripped)
                if m:
                    return m.group(1)
    raise RuntimeError("Telegram bot token not found in keys.toml")


def send_rich_message(text):
    """Send via Telegram's sendRichMessage API (Bot API 10.1) with HTML fallback."""
    token = get_bot_token()
    url = f"https://api.telegram.org/bot{token}/sendRichMessage"
    payload = json.dumps({
        "chat_id": TELEGRAM_CHAT_ID,
        "rich_message": {"markdown": text}
    }).encode("utf-8")
    req = urllib.request.Request(url, data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read())
            if data.get("ok"):
                msg_id = data["result"]["message_id"]
                print(f"✓ Delivered rich message (msg_id: {msg_id})")
                return
            print(f"✗ sendRichMessage returned ok=false: {data}")
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")
        print(f"✗ sendRichMessage failed ({e.code}): {body}")
    except Exception as e:
        print(f"✗ sendRichMessage error: {e}")

    # Fallback to sendMessage with HTML
    print("Falling back to HTML...")
    html = text
    html = re.sub(r'^### (.+)$', r'<b><i>\1</i></b>', html, flags=re.MULTILINE)
    html = re.sub(r'^## (.+)$', r'<b>\1</b>', html, flags=re.MULTILINE)
    html = re.sub(r'^# (.+)$', r'<b>\1</b>', html, flags=re.MULTILINE)
    html = re.sub(r'\*\*(.+?)\*\*', r'<b>\1</b>', html)
    html = re.sub(r'\*(.+?)\*', r'<i>\1</i>', html)
    url2 = f"https://api.telegram.org/bot{token}/sendMessage"
    payload2 = json.dumps({"chat_id": TELEGRAM_CHAT_ID, "text": html, "parse_mode": "HTML"}).encode("utf-8")
    req2 = urllib.request.Request(url2, data=payload2, headers={"Content-Type": "application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req2, timeout=30) as resp2:
            data2 = json.loads(resp2.read())
            if data2.get("ok"):
                print(f"✓ Delivered fallback (msg_id: {data2['result']['message_id']})")
    except Exception as e:
        print(f"✗ Fallback error: {e}")


# ============================================================================
# DATA COLLECTION
# ============================================================================

def clean_session_title(title):
    """Clean up session title for display."""
    if not title:
        return "Untitled"
    t = title
    if t.startswith("Telegram: "):
        t = t[10:]
    idx = t.find(" [chat:")
    if idx > 0:
        t = t[:idx]
    if t.startswith("DM "):
        t = t[3:].strip()
    if t.startswith("▶"):
        parts = t.replace("▶", "").strip().split("|")
        t = parts[0].strip()
    return t


def time_ago(timestamp_val):
    """Convert timestamp to human-readable 'Xh ago' format."""
    now = datetime.now(timezone.utc)
    if isinstance(timestamp_val, (int, float)):
        dt = datetime.fromtimestamp(timestamp_val, tz=timezone.utc)
    else:
        try:
            dt = datetime.fromisoformat(str(timestamp_val).replace("Z", "+00:00"))
        except (ValueError, TypeError):
            return "?"
    diff = now - dt
    hours = int(diff.total_seconds() // 3600)
    minutes = int((diff.total_seconds() % 3600) // 60)
    if hours > 24:
        days = hours // 24
        return f"{days}d ago"
    if hours > 0:
        return f"{hours}h{minutes:02d}m ago"
    return f"{minutes}m ago"


def get_github_stats():
    """Load latest snapshot from the GitHub stats JSON file."""
    try:
        with open(STATS_PATH, "r") as f:
            data = json.load(f)
        if data:
            return data[-1]
    except (FileNotFoundError, json.JSONDecodeError, IndexError):
        pass
    return None


def get_recent_sessions(db, hours=24, limit=10):
    """Get sessions active in the last N hours."""
    cutoff = int((datetime.now(timezone.utc) - timedelta(hours=hours)).timestamp())
    cursor = db.execute("""
        SELECT s.id, s.title, s.category, s.updated_at, s.token_count
        FROM sessions s
        WHERE s.updated_at > ?
        ORDER BY s.updated_at DESC
        LIMIT ?
    """, (cutoff, limit))
    return cursor.fetchall()


def get_last_user_message(db, session_id, max_length=80):
    """Get the last user message for a session, stripped of metadata."""
    cursor = db.execute("""
        SELECT content FROM messages
        WHERE session_id = ? AND role = 'user'
        ORDER BY created_at DESC
        LIMIT 1
    """, (session_id,))
    row = cursor.fetchone()
    if row and row[0]:
        text = row[0].strip()
        # Strip reasoning blocks and tool markers
        text = re.sub(r'<!--\s*reasoning\s*-->.*?<!--\s*/reasoning\s*-->', '', text, flags=re.DOTALL)
        text = re.sub(r'<!--\s*tools-v2:.*?-->', '', text)
        text = re.sub(r'<!--\s*/tools.*?-->', '', text)
        text = text.replace("\n", " ").strip()
        if len(text) > max_length:
            text = text[:max_length-3] + "..."
        return text if text else None
    return None


def get_cron_summary(db, hours=24):
    """Get cron job runs from the last N hours."""
    cursor = db.execute(f"""
        SELECT j.name, r.status, r.started_at, substr(r.content, 1, 120)
        FROM cron_job_runs r
        JOIN cron_jobs j ON j.id = r.job_id
        WHERE r.started_at > datetime('now', '-{hours} hours')
        ORDER BY r.started_at DESC
    """)
    return cursor.fetchall()


# ============================================================================
# FORMATTING
# ============================================================================

def format_recap(sessions, github_stats, cron_runs, db):
    """Format the morning recap as markdown."""
    today = datetime.now(timezone.utc).strftime("%A, %d %B %Y")
    parts = []

    # Header
    parts.append("# ☀️ Morning Recap")
    parts.append(f"📅 {today}")
    parts.append("")

    # GitHub Stats
    parts.append("## 📊 GitHub")
    if github_stats:
        s = github_stats
        parts.append("")
        parts.append("| Metric | Value |")
        parts.append("|--------|-------|")
        parts.append(f"| ⭐ Stars | {s.get('stars', '?')} |")
        parts.append(f"| 🍴 Forks | {s.get('forks', '?')} |")
        parts.append(f"| 📥 Total Clones | {s.get('clones_total', 0):,} |")
        parts.append(f"| 👁 Total Views | {s.get('views_total', 0):,} |")
        parts.append(f"| 📦 Clones (14d) | {s.get('clones_14d', '?')} |")
        parts.append(f"| 🔍 Views (14d) | {s.get('views_14d', '?')} |")
    else:
        parts.append("Stats unavailable")
    parts.append("")

    # Active Sessions
    parts.append("## 💬 Active Sessions (24h)")
    if sessions:
        for row in sessions:
            sid, title, category, updated_at, tokens = row
            name = clean_session_title(title)
            ago = time_ago(updated_at)
            tokens_m = f"{tokens / 1_000_000:.0f}M" if tokens >= 1_000_000 else f"{tokens / 1_000:.0f}K"
            meta = f"{category or 'General'} · {ago} · {tokens_m} tokens"
            parts.append(f"- **{name}** *({meta})*")
            last_msg = get_last_user_message(db, sid)
            if last_msg:
                parts.append(f"  ↳ {last_msg}")
    else:
        parts.append("No sessions in the last 24h")
    parts.append("")

    # Cron Health
    parts.append("## ⚙️ Cron Jobs (24h)")
    if cron_runs:
        passed = sum(1 for _, status, _, _ in cron_runs if status == "success")
        failed = sum(1 for _, status, _, _ in cron_runs if status != "success")
        parts.append("")
        parts.append("| Status | Count |")
        parts.append("|--------|-------|")
        parts.append(f"| ✅ Passed | {passed} |")
        parts.append(f"| ❌ Failed | {failed} |")
        if failed > 0:
            parts.append("")
            parts.append("**Failed:**")
            for name, status, started, content in cron_runs:
                if status != "success":
                    parts.append(f"- **{name}**: {status}")
    else:
        parts.append("No runs in the last 24h")

    return "\n".join(parts)


# ============================================================================
# MAIN
# ============================================================================

def main():
    """Generate and send the morning recap."""
    if TELEGRAM_CHAT_ID == "YOUR_CHAT_ID_HERE":
        print("✗ Error: TELEGRAM_CHAT_ID not set. Edit the script or set the environment variable.")
        return

    db = sqlite3.connect(DB_PATH)
    try:
        sessions = get_recent_sessions(db)
        github_stats = get_github_stats()
        cron_runs = get_cron_summary(db)

        recap = format_recap(sessions, github_stats, cron_runs, db)
        send_rich_message(recap)
    finally:
        db.close()


if __name__ == "__main__":
    main()
