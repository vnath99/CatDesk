# T-0483 R7 — Direct Wake store alias evidence

Date: 2026-10-14. Scope: read-only Windows file-handle inspection. No installed package, running process, Wake event, task, or tunnel was changed.

## Observed objects

The running installed WakeHost dev.84 process 69708 has a `host.lock` handle whose Windows volume/file-index identity is `c6de7348:014b0000:00028c3a`. Its `GetFinalPathNameByHandleW` display is `C:\Users\Volap\AppData\Local\CatDeskWake\host.lock`. A fresh read-only open of that displayed path from this Codex command environment yields `c6de7348:000f0000:00087816`. A fresh open of `C:\Users\Volap\AppData\Local\Packages\OpenAI.Codex_2p2nqsd0c76g0\LocalCache\Local\CatDeskWake\host.lock` yields the **same** `c6de7348:000f0000:00087816` object as the direct open. The direct candidate `store-provenance` command also yields the same root-object hash (`c70b11488090ea47b2c6bdfb5c751f846dd4b1f8441f4b0cde5fe482af814c28`) when `LOCALAPPDATA` is explicitly set to the package LocalCache root. Its bounded config and status SHA-256 values match that shadow store.

The connected CatDesk MCP readback from the preceding turn reported live dev.84/Chat53 generation34; the direct installed CLI still reports old dev.59/Chat52 generation1. On this read, the CatDesk local MCP connector returned a transport send error, so no new connected status was inferred. The local daemon itself still has a loopback health listener on port 3200 and returns `status: ok` to its public root endpoint; that health response does not prove Wake store authority or revive the MCP connector.

These observations prove the running host's open lock and the direct CLI's opened lock are **different file objects**, and that the direct command environment's lock is the Codex LocalCache object. A package filesystem view is a plausible explanation, but the mechanism and the live host's root identity are not proven by this evidence. A displayed path, equal `LOCALAPPDATA` environment text, and equal executable path are insufficient authority checks here.

## Release consequence

Do not use the direct CLI's old dev.59 status to overwrite, restart, or retire the live dev.84 host. The R6 source-only root-object hash provides a future comparable readback after a reviewed deployment, but the installed daemon does not expose it yet. Exact-commit GitHub Windows CI remains unavailable: `22ac87b` and `1ff560b` each have zero check runs, while explicit CI dispatch previously returned HTTP 422 (`Actions has been disabled for this repository`). Protected dev.85 activation and any new Chat53 canary remain held.
