# Long-Running Job Manager

Status: T-0021 implementation note
Date: 2026-07-25

## Purpose

The long-running job manager gives the delegated runtime a bounded way to start,
observe, recover, and cancel local commands without bypassing CatDesk command
policy.

CatDesk remains the execution authority:

- every job command is checked by the existing shell safety policy before spawn;
- every job has a durable `record.json`;
- stdout and stderr are written to local files;
- supervisor reads are bounded and cursor based;
- cancellation uses process-tree termination where the platform supports it;
- restart recovery marks missing running processes as `LOST`;
- log rotation keeps large output local instead of returning it wholesale.

## Durable Layout

Each job is stored under the configured job root:

```text
<job-root>/
  job-<uuid>/
    record.json
    stdout.log
    stdout.log.1
    stderr.log
    stderr.log.1
```

`record.json` stores:

- command, working directory, command profile, and log limit;
- process id when available;
- status and exit code;
- start and finish timestamps;
- local stdout and stderr paths;
- terminal status detail.

The current status values are:

- `RUNNING`
- `SUCCEEDED`
- `FAILED`
- `CANCELLED`
- `LOST`

## Log Polling

`poll_log` accepts a stream, byte offset, and maximum response size. The manager
caps reads at 64 KiB even if a larger value is requested. The response includes:

- returned text;
- original/effective offset;
- next offset;
- truncation flag;
- rotation flag;
- local log path.

If the caller supplies an offset beyond the current file length, the manager
treats that as evidence of rotation and resumes from offset zero on the current
file.

## Restart Recovery

`recover_running_jobs` loads durable records and checks whether recorded running
processes still exist. Missing processes are marked `LOST` with a recovery
detail. Running processes that are still alive remain `RUNNING` for later
supervisor action.

## Limits

T-0023A wires the job manager into the integrated delegated service tool
dispatcher through `job.start`, `job.status`, `job.poll`, and `job.cancel`.
The manager remains a bounded local primitive rather than a full production job
scheduler.
