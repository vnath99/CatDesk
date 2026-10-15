# T-0478 D3 — Read-only candidate owner recovery classifier

Date: 2026-10-15. Status: source-only forensic classification; no cleanup authority.

## Source change

The reviewed-candidate serving record and runtime-attention record now use schema 2 and bind the owner PID to its Windows process creation time. The retirement records still use their outer schema 1 while embedding the schema-2 serving record. The classifier takes one validated, witnessed snapshot manifest and opens only its exact derived journal/root names under the canonical temporary directory. It verifies the owner-only descriptors, root object binding, canonical bounded records and a journal inventory capped at seven entries. It does not scan arbitrary candidate names.

The diagnostic states are `NoJournal`, `IntentBeforeRoot`, `BoundBeforeServing`, `OwnerProcessAlive`, `OrphanedOwnerExited`, `OrphanedPidReused`, `RetiredJournalPresent` and `Attention`. A root without its journal, extra/changed records, incomplete retirement, mismatched object or uncertain process access reports attention. A previous schema-1 PID-only serving record also reports attention because PID alone cannot identify the process instance. For a serving record, the process check opens the exact PID for limited query/synchronization, compares process creation time, and checks the retained process handle's signaled state. Classification of an exited or reused PID is diagnostic evidence; it cannot prove that a root may be disposed.

## Local verification

The focused candidate snapshot suite passed 23 Windows tests at this checkpoint. New regressions exercise live, exited and reused process instances, exact journal phase classification without journal mutation, a legacy PID-only record, a foreign journal leaf, a retired journal with absent root, and a generated root without a journal. The full root all-features Rust suite exited 0 (main binary: 1,145 passed, 26 ignored; integration suites passed), as did strict all-target/all-feature Clippy and rustfmt. The printed AppContainer access-denied child process is expected negative test evidence; the outer suite passed. The exact new commit's independent three-job Windows CI still needs confirmation.

## Recovery boundary

No public recovery command calls the classifier yet. It never terminates or adopts a process, retires a journal, or deletes a root. A future reviewed recovery transaction must establish exclusive ownership, revalidate the pinned identities and complete a bounded disposition with independent CI and live evidence. Legacy schema-1 residue needs explicit reviewed handling; it is not silently promoted to owner proof. No successful CI-witnessed reviewed-candidate launch or production orphan recovery is accepted here. Installed WakeHost, supervisor service and tunnel remain unchanged.
