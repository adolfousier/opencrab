# MEMORY.md

Synthetic memory corpus for recall evaluation. Every entry is invented. No real
person, workspace, host or conversation appears here.

## Build commands

Use the linter with all features enabled, never the plain check or build
commands. Formatting runs separately before any commit.

## Release process

Releases are cut from tags only. A tag push starts the pipeline; pushing to the
default branch does not. Never draft a release unless it was asked for in the
current conversation.

## Push approval

Never push to a remote without being asked in the current conversation. Local
commits are fine at any time. Approval for one push does not carry to the next.

## Database migrations

Migrations are numbered and the count constant must be bumped in the same
change, otherwise the new file is silently skipped at startup.

## Voice notes

Reply with text first so the conversation stays searchable, then attach the
generated audio. Both belong in the thread.

## Group chat language

Answer in the language the person you are addressing wrote in. The language
most common in the room does not override the individual you are replying to.

## Screenshot handling

When an image arrives, look at it before running any other tool. Evidence in
hand outranks a theory about what the problem probably is.

## Timezone handling

Timestamps are stored in UTC and rendered in the viewer's local zone. Never
store a local timestamp; the offset is lost and cannot be recovered later.

## Retry and backoff

Transient failures retry with exponential backoff and a jitter term. A hard
authentication failure is not transient and must not be retried.

## Cache invalidation

Cached parses carry the source modification time and length. Comparing only the
timestamp misses a rewrite that lands inside the same second.

## Test placement

Every test lives in its own file under the tests directory and is registered in
the module list. Inline test modules inside source files are not used here.

## Error handling

Never discard a failed result silently. Even when the code proceeds anyway, the
failure gets logged with enough context to identify what was lost.
