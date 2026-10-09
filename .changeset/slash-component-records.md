---
default: patch
---

Preserve separate classic records whose component names contain slashes, including
browser-console-prefixed records. Count unsupported punctuated header-shaped
candidates as rejections instead of silently appending them to preceding records,
while preserving ordinary multiline payloads and tracebacks.
