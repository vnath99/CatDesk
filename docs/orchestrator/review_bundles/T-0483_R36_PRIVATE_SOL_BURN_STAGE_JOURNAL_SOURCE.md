# T-0483 R36 — Private Sol burn-to-stage journal (source only)

## Change

The host-only staging module now has a private, typed journal for one selected Sol output nonce. It writes a canonical create-once stage intent before burning the nonce, binds the task/turn/allowlist, worker manifest digest, output and stage-root physical identities, worker SID and one selected stage ID, then flushes the ledger parent. After the existing exact output verifier writes a create-once burn marker and flushes that parent, the private path stages the verified bytes at the selected ID, checks exact stage readback, flushes stage directories, and writes a bound completion record. The burn record is rechecked after completion readback.

A restart only diagnoses the intent, burn, completion and staged object; it always returns `ATTENTION`. It cannot reconstruct the one-time burned output, retry staging, or grant repository, review or Wake authority. The stopped-worker candidate does not call this journal. In particular, no fixed-account live worker or proof against copied writable handles and outside-Job processes feeds it.

## Verification

- Focused Windows staging tests passed 21/21, and nonce tests passed 11/11. They cover interrupted intent, burn, stage and completion steps; a reserved stage-ID collision; torn or changed records and stage bytes; and diagnostic-only reopen.
- Full root binary suite passed 1,242 tests with 28 ignored. Strict workspace all-targets/all-features Clippy, formatting and diff checks passed. A second independent read-only review of the final burn recheck and tests found no blocker. The combined isolated shell/cwd and remaining root all-features suite, including nested Wake, also passed after the source commits.

Source commit `da58c72634322d638291989fbca6465cabecbb6a` was pushed. Immediate exact-SHA GitHub readback found zero workflow runs and zero check-runs. No remote CI acceptance is claimed. File and parent flush tests simulate process interruption; they do not establish sudden-power-loss durability on the actual protected filesystem.

## Remaining gate

Provide the trusted fixed `CatDeskSolWorker` account/token and approved executable, prove all worker and outside-Job writers and copied writable handles have lost write authority, then connect a live stopped worker to this journal under host-owned ACLs. A durable repository-use receipt, genuine GPT-6.1 Sol/medium deliverable, natural `review_ready` event and exact Wake receipt still need independent acceptance. Under the October directive, this optional isolated worker lane does not block ordinary approved Codex CLI or safe multi-project source work.
