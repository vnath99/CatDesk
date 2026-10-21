#![recursion_limit = "256"]
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::get_first,
    clippy::identity_op,
    clippy::items_after_test_module,
    clippy::manual_clamp,
    clippy::manual_div_ceil,
    clippy::manual_contains,
    clippy::needless_borrows_for_generic_args,
    clippy::needless_lifetimes,
    clippy::question_mark,
    clippy::redundant_closure,
    clippy::some_filter,
    clippy::single_char_add_str,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::unnecessary_cast,
    clippy::upper_case_acronyms,
    clippy::useless_vec
)]

mod app_info;
mod binagotchy_cli;
mod binagotchy_gen;
mod browser;
mod command;
#[allow(dead_code)] // R1 foundation; host installer/bridge activation is R2.
mod control_plane_supervisor;
mod core_host_acceptance_preflight;
#[allow(dead_code)] // T-0311 authority primitive; T-0312 owns evaluator integration.
mod core_host_gate_approval;
#[allow(dead_code)] // T-0312 ledger; fixed capture remains the later T-0313 boundary.
mod core_host_gate_evidence;
mod daemon_reload;
mod daemon_reload_approval;
mod delegated;
mod devtools;
mod git_workflow;
#[allow(dead_code)] // R3B4-B1: fixed Git-object candidate reader, not a trusted producer.
mod isolated_fixture_git_objects;
#[allow(dead_code)] // Fixed live and diagnostic GitHub witnesses; not production signing authority.
mod isolated_fixture_github_authority;
#[allow(dead_code)]
// T0478 R3B4-A pure schema; trusted producer and runtime enforcement follow later.
mod isolated_fixture_manifest;
#[allow(dead_code)]
// R3B4-B3 dormant handle-pinned disposable source staging, no runtime MCP binding.
mod isolated_fixture_snapshot;
mod macos_terminal;
mod mascot;
mod mcp;
mod ngrok;
mod openai_tunnel;
mod operator_codex_cli;
mod operator_facade;
#[allow(dead_code)] // T-0381 fixed reviewed artifact reader; bootstrap remains separate.
mod ordinary_worker_artifact_provider;
#[allow(dead_code)] // T-0387 typed review authority; pair provisioning remains separate.
mod ordinary_worker_pair_approval;
mod planning;
mod project_memory;
mod prompt_templates;
mod repo_map;
mod reviewed_build;
mod reviewed_candidate_owner;
mod reviewed_source_snapshot;
mod server;
mod stable_wake_adapter_runtime;
mod stable_wake_bootstrap;
mod stable_wake_core;
#[allow(dead_code)] // R1B-R1 durable transition foundation; browser owner cutover is deferred.
mod stable_wake_delivery;
#[allow(dead_code)] // T-0253 source-only dormant owner; activation is T-0254.
mod stable_wake_owner;
mod stable_wake_owner_mode;
mod state;
mod supervisor_lifecycle;
mod task_queue;
mod theme;
mod tunnel;
#[allow(dead_code)] // T-0374 foundation; host-adapter cutover remains separate.
mod user_worker_release;
mod verification;
mod wake_protocol_client;
mod web_context_control;
#[allow(dead_code)] // Advisory Web conversation meter; operator/browser integration follows.
mod web_conversation_context;
#[cfg(all(test, windows))]
mod web_development_tests;
mod web_work;
mod web_work_state;
mod windows_gui;
mod windows_protected_fs;
#[allow(dead_code)]
mod windows_supervisor_control_pipe;
#[allow(dead_code)]
mod windows_supervisor_startup;
mod workspace_tools;

use crossterm::{
    ExecutableCommand,
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
    },
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use delegated::autonomy_observability::read_snapshot as read_autonomy_snapshot;
use devtools::DevtoolsBridge;
use mascot::{TUI_MASCOT_BLOCK_HEIGHT, TUI_MASCOT_BLOCK_WIDTH, render_tui_lines};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use state::{
    AppState, FLOW_ANIM_CELLS, FLOW_BOOTSTRAP_PHASES, FlowAnimKind, FlowAnimSegment, FlowDirection,
    FlowLane, Mode, ServerUiEvent, SharedState, ToolMode, UsageTotals, app_config_path,
    flow_anim_lit_count, load_ngrok_authtoken, save_ngrok_authtoken,
};
use std::io::{Write, stdout};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{
    Mutex,
    mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
};

const FLOW_ROW_CELLS: usize = FLOW_ANIM_CELLS;
const FLOW_LANE_LEFT_LABEL: &str = "Your computer ";
const REMOTE_CONNECT_UI_GRACE_MS: u128 = 8_000;
const UI_POLL_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);
const AUTONOMY_REFRESH_INTERVAL: Duration = Duration::from_millis(750);
const STATUS_PANEL_HEIGHT: u16 = TUI_MASCOT_BLOCK_HEIGHT + 13;
const STATUS_LABEL_WIDTH: usize = 19;
const GPT55_INPUT_USD_PER_1M: f64 = 5.0;
const GPT55_OUTPUT_USD_PER_1M: f64 = 30.0;
const PRICE_DISPLAY_DECIMALS: usize = 6;
const NGROK_SETUP_URL: &str = "https://dashboard.ngrok.com/get-started/setup";

// ── Selection ───────────────────────────────────────────────

struct Selection {
    start: Option<(u16, u16)>,
    end: Option<(u16, u16)>,
    dragging: bool,
}

impl Selection {
    fn new() -> Self {
        Self {
            start: None,
            end: None,
            dragging: false,
        }
    }
    fn clear(&mut self) {
        self.start = None;
        self.end = None;
        self.dragging = false;
    }
    fn range(&self) -> Option<((u16, u16), (u16, u16))> {
        match (self.start, self.end) {
            (Some(s), Some(e)) => {
                let (r0, c0, r1, c1) = if (s.1, s.0) <= (e.1, e.0) {
                    (s.1, s.0, e.1, e.0)
                } else {
                    (e.1, e.0, s.1, s.0)
                };
                Some(((c0, r0), (c1, r1)))
            }
            _ => None,
        }
    }
}

fn extract_from_screen(lines: &[String], start: (u16, u16), end: (u16, u16)) -> String {
    let (c0, r0) = start;
    let (c1, r1) = end;
    let mut result = String::new();
    for row in r0..=r1 {
        let idx = row as usize;
        if idx >= lines.len() {
            break;
        }
        let line: Vec<char> = lines[idx].chars().collect();
        let cs = if row == r0 { c0 as usize } else { 0 };
        let ce = if row == r1 {
            (c1 as usize).min(line.len().saturating_sub(1))
        } else {
            line.len().saturating_sub(1)
        };
        for col in cs..=ce {
            if col < line.len() {
                result.push(line[col]);
            }
        }
        if row != r1 {
            result.push('\n');
        }
    }
    result
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

fn current_anim_segment(flow: &FlowLane, now_millis: u128) -> Option<FlowAnimSegment> {
    if let Some(seg) = flow
        .anim_queue
        .iter()
        .find(|seg| seg.started_ms <= now_millis && now_millis < seg.ends_ms)
    {
        return Some(*seg);
    }
    flow.anim_queue.front().copied()
}

fn should_display_flow_row(flow: &FlowLane, remote_connected: bool) -> bool {
    remote_connected || flow.closing_started_ms.is_some() || !flow.anim_queue.is_empty()
}

fn flow_direction(flow: Option<&FlowLane>, now_millis: u128) -> FlowDirection {
    if let Some(flow) = flow {
        if let Some(seg) = current_anim_segment(flow, now_millis) {
            return seg.direction;
        }
        return flow.last_direction;
    }
    FlowDirection::Forward
}

fn flow_lit_count(flow: Option<&FlowLane>, now_millis: u128, cells: usize) -> usize {
    let Some(flow) = flow else {
        return 0;
    };
    if flow.closing_started_ms.is_some() {
        return 0;
    }
    current_anim_segment(flow, now_millis)
        .map(|seg| flow_anim_lit_count(seg, now_millis).min(cells))
        .unwrap_or(0)
}

fn debug_lane(direction: Option<FlowDirection>, lit_count: usize, cells: usize) -> String {
    let mut out = String::with_capacity(cells);
    for i in 0..cells {
        let lit_here = match direction {
            Some(FlowDirection::Forward) => lit_count > 0 && i < lit_count,
            Some(FlowDirection::Backward) => lit_count > 0 && i >= cells.saturating_sub(lit_count),
            None => false,
        };
        out.push(if lit_here { '#' } else { '-' });
    }
    out
}

fn flow_lane_spans(
    active: bool,
    flow: Option<&FlowLane>,
    palette: &theme::Palette,
    now_millis: u128,
) -> Vec<Span<'static>> {
    const CELLS: usize = FLOW_ROW_CELLS;
    let unlit = Style::default().fg(palette.muted_fg);
    let lit = Style::default()
        .fg(palette.info_fg)
        .add_modifier(Modifier::BOLD);

    let direction = flow.map(|flow| flow_direction(Some(flow), now_millis));
    let lit_count = if active {
        flow_lit_count(flow, now_millis, CELLS)
    } else {
        0
    };

    if lit_count == 0 || direction.is_none() {
        return vec![Span::styled("-".repeat(CELLS), unlit), Span::raw(" ")];
    }

    let direction = direction.unwrap_or(FlowDirection::Forward);
    let mut spans = Vec::with_capacity(CELLS + 1);
    for i in 0..CELLS {
        let lit_here = match direction {
            FlowDirection::Forward => i < lit_count,
            FlowDirection::Backward => i >= CELLS.saturating_sub(lit_count),
        };
        let style = if lit_here { lit } else { unlit };
        spans.push(Span::styled("-".to_string(), style));
    }
    spans.push(Span::raw(" "));
    spans
}

fn trim_line(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    let kept = chars[..max_chars.saturating_sub(3)]
        .iter()
        .collect::<String>();
    format!("{kept}...")
}

fn compact_duration(millis: u128) -> String {
    let seconds = millis / 1_000;
    if seconds >= 3_600 {
        format!("{}h{:02}m", seconds / 3_600, (seconds % 3_600) / 60)
    } else if seconds >= 60 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

fn format_token_compact(value: u64) -> String {
    if value < 1_000 {
        return value.to_string();
    }

    let (unit, suffix) = if value >= 1_000_000_000 {
        (1_000_000_000.0, "B")
    } else if value >= 1_000_000 {
        (1_000_000.0, "M")
    } else {
        (1_000.0, "K")
    };
    let scaled = value as f64 / unit;
    let decimals = if scaled >= 100.0 { 0 } else { 1 };
    let formatted = format!("{scaled:.prec$}", prec = decimals);
    format!("{}{}", formatted.trim_end_matches(".0"), suffix)
}

fn estimate_gpt55_usage_cost_usd(usage: &UsageTotals) -> f64 {
    (usage.input_tokens as f64 * GPT55_OUTPUT_USD_PER_1M
        + usage.output_tokens as f64 * GPT55_INPUT_USD_PER_1M)
        / 1_000_000.0
}

fn format_usd_compact(usd: f64) -> String {
    let formatted = format!("{usd:.prec$}", prec = PRICE_DISPLAY_DECIMALS);
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn session_usage_line(
    usage: &UsageTotals,
    status_label: Span<'static>,
    palette: &theme::Palette,
) -> Line<'static> {
    let label_style = Style::default().fg(palette.muted_fg);
    let value_style = Style::default()
        .fg(palette.secondary_fg)
        .add_modifier(Modifier::BOLD);
    let price_style = Style::default()
        .fg(palette.success_fg)
        .add_modifier(Modifier::BOLD);

    Line::from(vec![
        status_label,
        Span::styled("↓", label_style),
        Span::styled(format_token_compact(usage.input_tokens), value_style),
        Span::raw("  "),
        Span::styled("↑", label_style),
        Span::styled(format_token_compact(usage.output_tokens), value_style),
        Span::raw("  "),
        Span::styled("Σ", label_style),
        Span::styled(format_token_compact(usage.total_tokens), value_style),
        Span::raw("  "),
        Span::styled("ƒ", label_style),
        Span::styled(format_token_compact(usage.tool_call_count), value_style),
        Span::raw("  "),
        Span::styled("$", label_style),
        Span::styled(
            format_usd_compact(estimate_gpt55_usage_cost_usd(usage)),
            price_style,
        ),
    ])
}

fn flow_call_offset(text: &str) -> String {
    let text_width = text.chars().count();
    let centered_in_lane = FLOW_ROW_CELLS.saturating_sub(text_width) / 2;
    " ".repeat(FLOW_LANE_LEFT_LABEL.len() + centered_in_lane)
}

fn flow_phase(flow: &FlowLane, now_millis: u128) -> &'static str {
    if flow.closing_started_ms.is_some() {
        return "close";
    }
    if let Some(seg) = current_anim_segment(flow, now_millis) {
        return match seg.kind {
            FlowAnimKind::Turn => "turn",
            FlowAnimKind::Move => match seg.direction {
                FlowDirection::Forward => "request",
                FlowDirection::Backward => "response",
            },
        };
    }
    "idle"
}

fn latest_flow_action(flow: &FlowLane) -> String {
    flow.events
        .iter()
        .rev()
        .find_map(|event| {
            if let Some(tool) = event.strip_prefix("tools/call:") {
                if tool.is_empty() {
                    None
                } else {
                    Some(tool.to_string())
                }
            } else if event.is_empty() {
                None
            } else {
                Some(event.clone())
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlowPhaseStepState {
    Future,
    Pending,
    Complete,
}

fn flow_phase_bounds(phase_index: usize) -> (usize, usize) {
    let start = FLOW_BOOTSTRAP_PHASES
        .iter()
        .take(phase_index)
        .map(|phase| phase.steps.len())
        .sum::<usize>();
    let end = start + FLOW_BOOTSTRAP_PHASES[phase_index].steps.len();
    (start, end)
}

fn flow_phase_step_state(flow: Option<&FlowLane>, step_index: usize) -> FlowPhaseStepState {
    let Some(flow) = flow else {
        return FlowPhaseStepState::Future;
    };
    if step_index < flow.bootstrap_completed_steps {
        FlowPhaseStepState::Complete
    } else if flow.bootstrap_pending_steps.contains(&step_index) {
        FlowPhaseStepState::Pending
    } else {
        FlowPhaseStepState::Future
    }
}

fn flow_phase_status_label(flow: Option<&FlowLane>, phase_index: usize) -> Option<String> {
    let Some(flow) = flow else {
        return None;
    };
    let (start, end) = flow_phase_bounds(phase_index);
    if flow.bootstrap_completed_steps >= end {
        return Some("✓".to_string());
    }
    if let Some(step_index) = flow
        .bootstrap_pending_steps
        .iter()
        .copied()
        .find(|step_index| (start..end).contains(step_index))
    {
        let step = &FLOW_BOOTSTRAP_PHASES[phase_index].steps[step_index - start];
        return Some(step.label.to_string());
    }
    if (start..end).contains(&flow.bootstrap_completed_steps.saturating_sub(1))
        && flow.bootstrap_completed_steps > start
    {
        let step_index = flow.bootstrap_completed_steps - 1;
        let step = &FLOW_BOOTSTRAP_PHASES[phase_index].steps[step_index - start];
        return Some(step.label.to_string());
    }
    None
}

fn flow_phase_lines(
    flow: Option<&FlowLane>,
    palette: &theme::Palette,
    status_style: Style,
) -> Vec<Line<'static>> {
    const TITLE_STATUS_GAP: usize = 4;
    const STATUS_ANIM_GAP: usize = 4;
    let title_width = FLOW_BOOTSTRAP_PHASES
        .iter()
        .enumerate()
        .map(|(phase_index, phase)| format!("    Phase {}: {}", phase_index + 1, phase.title))
        .map(|title| title.chars().count())
        .max()
        .unwrap_or(0);
    let status_width = FLOW_BOOTSTRAP_PHASES
        .iter()
        .flat_map(|phase| {
            std::iter::once("✓".to_string())
                .chain(phase.steps.iter().map(|step| step.label.to_string()))
                .map(|status| format!("[{status}]").chars().count())
        })
        .max()
        .unwrap_or(0);
    let pending_style = Style::default()
        .fg(palette.info_fg)
        .add_modifier(Modifier::BOLD);
    let complete_style = Style::default()
        .fg(palette.success_fg)
        .add_modifier(Modifier::BOLD);
    let future_style = Style::default().fg(palette.muted_fg);
    let label_style = Style::default().fg(palette.primary_fg);

    FLOW_BOOTSTRAP_PHASES
        .iter()
        .enumerate()
        .map(|(phase_index, phase)| {
            let title = format!("    Phase {}: {}", phase_index + 1, phase.title);
            let title_padding = title_width.saturating_sub(title.chars().count());
            let status_label = flow_phase_status_label(flow, phase_index);
            let status_text = status_label
                .map(|label| format!("[{label}]"))
                .unwrap_or_default();
            let status_padding = status_width.saturating_sub(status_text.chars().count());
            let mut spans = vec![
                Span::styled(title, label_style),
                Span::styled(" ".repeat(title_padding + TITLE_STATUS_GAP), future_style),
                Span::styled(status_text, status_style),
                Span::styled(" ".repeat(status_padding + STATUS_ANIM_GAP), future_style),
            ];
            let (start, _) = flow_phase_bounds(phase_index);
            for (step_offset, _) in phase.steps.iter().enumerate() {
                if step_offset > 0 {
                    spans.push(Span::raw(" "));
                }
                let step_index = start + step_offset;
                match flow_phase_step_state(flow, step_index) {
                    FlowPhaseStepState::Future => {
                        spans.push(Span::styled("✧", future_style));
                    }
                    FlowPhaseStepState::Pending => {
                        spans.push(Span::styled("✧", pending_style));
                    }
                    FlowPhaseStepState::Complete => {
                        spans.push(Span::styled("✦", complete_style));
                    }
                }
            }
            Line::from(spans)
        })
        .collect()
}

fn flow_bootstrap_steps_total() -> usize {
    FLOW_BOOTSTRAP_PHASES
        .iter()
        .map(|phase| phase.steps.len())
        .sum()
}

fn flow_bootstrap_complete(flow: &FlowLane) -> bool {
    flow.bootstrap_completed_steps >= flow_bootstrap_steps_total()
        && flow.bootstrap_pending_steps.is_empty()
}

fn flow_bootstrap_status_visible(flow: &FlowLane, now_millis: u128) -> bool {
    if !flow_bootstrap_complete(flow) {
        return true;
    }
    if current_anim_segment(flow, now_millis).is_some() {
        return true;
    }
    flow.bootstrap_status_close_deadline_ms
        .is_some_and(|deadline| now_millis < deadline)
}

fn flow_bootstrap_countdown_remaining_seconds(flow: &FlowLane, now_millis: u128) -> Option<u128> {
    let deadline = flow.bootstrap_status_close_deadline_ms?;
    if now_millis >= deadline {
        return Some(0);
    }
    Some((deadline.saturating_sub(now_millis) + 999) / 1000)
}

fn active_bootstrap_status_flow<'a>(app: &'a AppState, now_millis: u128) -> Option<&'a FlowLane> {
    app.flows.iter().find(|flow| {
        should_display_flow_row(flow, app.remote_connected)
            && flow.bootstrap_status_active
            && flow.closing_started_ms.is_none()
            && flow_bootstrap_status_visible(flow, now_millis)
    })
}

fn should_show_connect_guide(app: &AppState, now_millis: u128) -> bool {
    let both_running = app.server_running && public_transport_available(app);
    let has_url = app.ngrok_url.is_some();
    let visible_flow_count = app
        .flows
        .iter()
        .filter(|flow| should_display_flow_row(flow, app.remote_connected))
        .count() as u16;
    let within_connect_grace = app
        .last_remote_activity_ms
        .map(|t| now_millis.saturating_sub(t) < REMOTE_CONNECT_UI_GRACE_MS)
        .unwrap_or(false);
    both_running
        && has_url
        && !app.remote_connected
        && visible_flow_count == 0
        && !within_connect_grace
}

fn public_transport_available(app: &AppState) -> bool {
    app.ngrok_url.is_some()
        && (app.ngrok_running
            || matches!(
                app.tunnel_config.mode,
                crate::tunnel::TunnelMode::ExternalTunnel
            ))
}

fn flow_bootstrap_status_lines(
    app: &AppState,
    flow: &FlowLane,
    palette: &theme::Palette,
    now_millis: u128,
) -> Vec<Line<'static>> {
    let action_label = latest_flow_action(flow);
    let bootstrap_complete = flow_bootstrap_complete(flow);
    let header_title = if bootstrap_complete {
        "Initialize completed"
    } else {
        "Initialize connector in progress"
    };
    let call_text = trim_line(&format!("call {action_label}"), FLOW_ROW_CELLS);
    let call_offset = flow_call_offset(&call_text);

    let mut lines = vec![
        Line::from(Span::styled(
            format!("  {header_title}"),
            Style::default()
                .fg(palette.title_fg)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", Style::default().fg(palette.muted_fg)),
            Span::styled(call_offset, Style::default().fg(palette.muted_fg)),
            Span::styled(
                call_text,
                Style::default()
                    .fg(palette.info_fg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from({
            let computer_role_style = Style::default()
                .fg(if app.server_running {
                    palette.success_fg
                } else {
                    palette.muted_fg
                })
                .add_modifier(Modifier::BOLD);
            let chatgpt_role_style = Style::default()
                .fg(if app.remote_connected {
                    palette.success_fg
                } else {
                    palette.muted_fg
                })
                .add_modifier(Modifier::BOLD);
            let mut row = vec![Span::styled(
                format!("  {FLOW_LANE_LEFT_LABEL}"),
                computer_role_style,
            )];
            row.extend(flow_lane_spans(true, Some(flow), palette, now_millis));
            row.push(Span::styled("ChatGPT Web", chatgpt_role_style));
            row
        }),
        Line::from(""),
    ];
    lines.extend(flow_phase_lines(
        Some(flow),
        palette,
        Style::default()
            .fg(palette.info_fg)
            .add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::from(""));

    let footer_text = if bootstrap_complete && current_anim_segment(flow, now_millis).is_none() {
        match flow_bootstrap_countdown_remaining_seconds(flow, now_millis) {
            Some(0) => "Completed.".to_string(),
            Some(seconds) => format!("Completed. Closing in {seconds}s..."),
            None => "Completed.".to_string(),
        }
    } else {
        "Auto closes after initialize is completed.".to_string()
    };
    lines.push(Line::from(Span::styled(
        format!("  {footer_text}"),
        Style::default().fg(palette.muted_fg),
    )));
    lines
}

fn build_animation_snapshot(app: &AppState) -> Vec<String> {
    if app.flows.is_empty() {
        return Vec::new();
    }
    let now_millis = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut rows = Vec::new();
    for flow in app
        .flows
        .iter()
        .filter(|flow| should_display_flow_row(flow, app.remote_connected))
    {
        let latest_action = latest_flow_action(flow);
        let closing = flow.closing_started_ms.is_some();
        let lane_active = closing
            || !flow.anim_queue.is_empty()
            || (app.server_running && public_transport_available(app) && app.remote_connected);
        let direction = Some(flow_direction(Some(flow), now_millis)).filter(|_| lane_active);
        let phase = flow_phase(flow, now_millis);
        let lit = flow_lit_count(Some(flow), now_millis, FLOW_ROW_CELLS);
        let lane = debug_lane(direction, lit, FLOW_ROW_CELLS);
        rows.push(format!(
            "flow {} phase={:<8} tool={:<16} Your computer {} ChatGPT Web (via Ngrok)",
            flow.short_id, phase, latest_action, lane
        ));
    }
    if rows.is_empty() {
        return Vec::new();
    }
    rows
}

#[cfg(target_os = "macos")]
fn clipboard_copy(text: &str) -> bool {
    let mut child = match std::process::Command::new("/usr/bin/pbcopy")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.wait();
        return false;
    };

    if stdin.write_all(text.as_bytes()).is_err() {
        drop(stdin);
        let _ = child.wait();
        return false;
    }

    drop(stdin);

    match child.wait() {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

#[cfg(target_os = "windows")]
fn clipboard_copy(text: &str) -> bool {
    let mut child = match std::process::Command::new("clip.exe")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.wait();
        return false;
    };

    if stdin.write_all(text.as_bytes()).is_err() {
        drop(stdin);
        let _ = child.wait();
        return false;
    }

    drop(stdin);

    match child.wait() {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn clipboard_copy(text: &str) -> bool {
    use base64::Engine as _;

    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let mut out = stdout();
    write!(out, "\x1b]52;c;{encoded}\x07")
        .and_then(|_| out.flush())
        .is_ok()
}

#[cfg(target_os = "macos")]
fn clipboard_paste() -> Option<String> {
    let output = std::process::Command::new("/usr/bin/pbpaste")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .filter(|text| !text.is_empty())
}

#[cfg(target_os = "windows")]
fn clipboard_paste() -> Option<String> {
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", "Get-Clipboard -Raw"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .filter(|text| !text.is_empty())
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn clipboard_paste() -> Option<String> {
    const CLIPBOARD_COMMANDS: &[(&str, &[&str])] = &[
        ("wl-paste", &["-n"]),
        ("xclip", &["-selection", "clipboard", "-o"]),
        ("xsel", &["--clipboard", "--output"]),
    ];

    for (program, args) in CLIPBOARD_COMMANDS {
        let output = match std::process::Command::new(program)
            .args(*args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };

        if let Ok(text) = String::from_utf8(output.stdout) {
            if !text.is_empty() {
                return Some(text);
            }
        }
    }

    None
}

fn key_is_clipboard_paste(key: &crossterm::event::KeyEvent) -> bool {
    matches!(key.code, KeyCode::Insert) && key.modifiers.contains(KeyModifiers::SHIFT)
        || matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&'v'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn normalize_ngrok_authtoken_input(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if let Some(idx) = parts.iter().position(|part| *part == "add-authtoken") {
        if let Some(token) = parts.get(idx + 1) {
            return token.trim_matches(['"', '\'']).to_string();
        }
    }

    trimmed.to_string()
}

fn drain_server_ui_events(app: &mut AppState, ui_events: &mut UnboundedReceiver<ServerUiEvent>) {
    while let Ok(event) = ui_events.try_recv() {
        app.apply_server_ui_event(event);
    }
}

struct HeadlessMcpOptions {
    host: String,
    port: u16,
    workspace: String,
    mcp_path: Option<String>,
    mode: Mode,
    tool_mode: ToolMode,
    config_path: PathBuf,
    auth_token: String,
    isolated_dev_fixture: bool,
    maximum_runtime_seconds: Option<u64>,
    web_auto_projects: std::collections::BTreeSet<String>,
}

struct ReviewedCandidateMcpOptions {
    source_workspace: PathBuf,
    expected_commit: String,
    auth_token: String,
}

struct ReviewedCandidateOwnerDiagnoseOptions {
    source_workspace: PathBuf,
    expected_commit: String,
    snapshot_id: String,
}

fn next_candidate_owner_diagnostic_value(
    args: &[String],
    index: &mut usize,
    option: &str,
) -> Result<String, String> {
    let value = next_arg(args, index, option)?;
    if value.is_empty() || value.starts_with('-') {
        return Err(format!("{option} requires a non-flag value"));
    }
    Ok(value)
}

fn parse_reviewed_candidate_owner_diagnose_options(
    args: &[String],
) -> Result<Option<ReviewedCandidateOwnerDiagnoseOptions>, String> {
    if !args
        .iter()
        .any(|arg| arg == "--reviewed-candidate-owner-diagnose")
    {
        return Ok(None);
    }
    let mut marker_seen = false;
    let mut workspace = None;
    let mut commit = None;
    let mut snapshot_id = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--reviewed-candidate-owner-diagnose" if !marker_seen => {
                marker_seen = true;
                index += 1;
            }
            "--source-workspace" if workspace.is_none() => {
                workspace = Some(next_candidate_owner_diagnostic_value(
                    args,
                    &mut index,
                    "--source-workspace",
                )?)
            }
            "--expected-commit" if commit.is_none() => {
                commit = Some(next_candidate_owner_diagnostic_value(
                    args,
                    &mut index,
                    "--expected-commit",
                )?)
            }
            "--snapshot-id" if snapshot_id.is_none() => {
                snapshot_id = Some(next_candidate_owner_diagnostic_value(
                    args,
                    &mut index,
                    "--snapshot-id",
                )?)
            }
            other => {
                return Err(format!(
                    "unknown candidate owner diagnostic option `{other}`"
                ));
            }
        }
    }
    if !marker_seen {
        return Err("candidate owner diagnosis mode marker was not parsed".into());
    }
    let source_workspace = workspace
        .filter(|value: &String| !value.is_empty())
        .map(PathBuf::from)
        .ok_or("candidate owner diagnosis requires source workspace")?;
    let expected_commit = commit.ok_or("candidate owner diagnosis requires expected commit")?;
    if expected_commit.len() != 40
        || !expected_commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("candidate owner diagnosis commit must be a lowercase full SHA-1".into());
    }
    let snapshot_id = snapshot_id.ok_or("candidate owner diagnosis requires snapshot ID")?;
    if uuid::Uuid::parse_str(&snapshot_id)
        .map(|value| value.to_string() != snapshot_id)
        .unwrap_or(true)
    {
        return Err("candidate owner diagnosis snapshot ID must be canonical".into());
    }
    Ok(Some(ReviewedCandidateOwnerDiagnoseOptions {
        source_workspace,
        expected_commit,
        snapshot_id,
    }))
}

fn parse_reviewed_candidate_mcp_options(
    args: impl IntoIterator<Item = String>,
) -> Result<Option<ReviewedCandidateMcpOptions>, String> {
    let args: Vec<String> = args.into_iter().collect();
    if !args.iter().any(|arg| arg == "--reviewed-candidate-mcp") {
        return Ok(None);
    }
    let mut marker_seen = false;
    let mut host = None;
    let mut port = None;
    let mut workspace = None;
    let mut commit = None;
    let mut token = None;
    let mut path = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--reviewed-candidate-mcp" if !marker_seen => {
                marker_seen = true;
                index += 1;
            }
            "--host" if host.is_none() => host = Some(next_arg(&args, &mut index, "--host")?),
            "--port" if port.is_none() => port = Some(next_arg(&args, &mut index, "--port")?),
            "--workspace" if workspace.is_none() => {
                workspace = Some(next_arg(&args, &mut index, "--workspace")?)
            }
            "--expected-commit" if commit.is_none() => {
                commit = Some(next_arg(&args, &mut index, "--expected-commit")?)
            }
            "--auth-token" if token.is_none() => {
                token = Some(next_arg(&args, &mut index, "--auth-token")?)
            }
            "--mcp-path" if path.is_none() => {
                path = Some(next_arg(&args, &mut index, "--mcp-path")?)
            }
            other => return Err(format!("unknown reviewed candidate option `{other}`")),
        }
    }
    if host.as_deref() != Some("127.0.0.1")
        || port.as_deref() != Some("0")
        || path.as_deref() != Some("/dev/mcp")
    {
        return Err("reviewed candidate requires explicit 127.0.0.1 port 0 and /dev/mcp".into());
    }
    let source_workspace = workspace
        .filter(|value: &String| !value.is_empty())
        .map(PathBuf::from)
        .ok_or("reviewed candidate requires explicit source workspace")?;
    let expected_commit = commit.ok_or("reviewed candidate requires expected commit")?;
    if expected_commit.len() != 40
        || !expected_commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("reviewed candidate commit must be a lowercase full SHA-1".into());
    }
    let auth_token = token.ok_or("reviewed candidate requires process-scoped auth token")?;
    if auth_token.len() < 24 || auth_token.chars().any(char::is_whitespace) {
        return Err("reviewed candidate auth token is unsafe".into());
    }
    Ok(Some(ReviewedCandidateMcpOptions {
        source_workspace,
        expected_commit,
        auth_token,
    }))
}

fn parse_reviewed_candidate_owner_options(
    args: &[String],
) -> Result<Option<ReviewedCandidateMcpOptions>, String> {
    if !args.iter().any(|arg| arg == "--reviewed-candidate-owner") {
        return Ok(None);
    }
    // The parent accepts the exact same fixed scope as the child. Reuse its
    // strict parser so aliases, duplicate options and wider binds fail closed.
    let child_args = args
        .iter()
        .map(|arg| {
            if arg == "--reviewed-candidate-owner" {
                "--reviewed-candidate-mcp".to_owned()
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>();
    parse_reviewed_candidate_mcp_options(child_args)
}

fn parse_headless_mcp_options(
    args: impl IntoIterator<Item = String>,
) -> Result<Option<HeadlessMcpOptions>, String> {
    let args: Vec<String> = args.into_iter().collect();
    if !args.iter().any(|arg| arg == "--headless-mcp") {
        return Ok(None);
    }

    let mut host = "127.0.0.1".to_string();
    let mut port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3200);
    let mut workspace = std::env::var("WORKSPACE_ROOT")
        .ok()
        .unwrap_or_else(|| ".".to_string());
    let mut mcp_path = None;
    let mut mode = Mode::Computer;
    let mut tool_mode = ToolMode::ReadOnly;
    let mut config_path: Option<PathBuf> = None;
    let mut auth_token = std::env::var("CATDESK_MCP_AUTH_TOKEN").ok();
    let mut isolated_dev_fixture = false;
    let mut maximum_runtime_seconds = None;
    let mut web_auto_projects = std::collections::BTreeSet::new();

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--headless-mcp" | "--no-ngrok" => {
                index += 1;
            }
            "--isolated-dev-fixture" => {
                if isolated_dev_fixture {
                    return Err("duplicate --isolated-dev-fixture".into());
                }
                isolated_dev_fixture = true;
                index += 1;
            }
            "--max-runtime-seconds" => {
                let value = next_arg(&args, &mut index, "--max-runtime-seconds")?;
                let seconds = value
                    .parse::<u64>()
                    .ok()
                    .filter(|seconds| (1..=900).contains(seconds))
                    .ok_or("--max-runtime-seconds requires 1..900")?;
                if maximum_runtime_seconds.replace(seconds).is_some() {
                    return Err("duplicate --max-runtime-seconds".into());
                }
            }
            "--web-auto-project" => {
                let project = next_arg(&args, &mut index, "--web-auto-project")?;
                if !catdesk_wake::protocol::valid_id(&project) || !web_auto_projects.insert(project)
                {
                    return Err("invalid or duplicate --web-auto-project".into());
                }
            }
            "--host" => {
                host = next_arg(&args, &mut index, "--host")?;
            }
            "--port" => {
                let value = next_arg(&args, &mut index, "--port")?;
                port = value
                    .parse::<u16>()
                    .map_err(|_| format!("invalid --port value `{value}`"))?;
            }
            "--workspace" => {
                workspace = next_arg(&args, &mut index, "--workspace")?;
            }
            "--mcp-path" => {
                mcp_path = Some(validate_headless_mcp_path(&next_arg(
                    &args,
                    &mut index,
                    "--mcp-path",
                )?)?);
            }
            "--mode" => {
                let value = next_arg(&args, &mut index, "--mode")?;
                mode = parse_headless_mode(&value)?;
            }
            "--tool-mode" => {
                let value = next_arg(&args, &mut index, "--tool-mode")?;
                tool_mode = parse_headless_tool_mode(&value)?;
            }
            "--config-path" => {
                config_path = Some(PathBuf::from(next_arg(&args, &mut index, "--config-path")?));
            }
            "--auth-token" => {
                auth_token = Some(next_arg(&args, &mut index, "--auth-token")?);
            }
            other => return Err(format!("unknown headless MCP option `{other}`")),
        }
    }

    validate_loopback_host(&host)?;
    if mode.browser_enabled() {
        return Err("headless MCP currently supports --mode computer only".into());
    }
    let config_path = config_path.ok_or_else(|| {
        "headless MCP requires --config-path so tests do not read or write the normal CatDesk config"
            .to_string()
    })?;
    let auth_token = auth_token
        .filter(|token| token.len() >= 24 && !token.chars().any(char::is_whitespace))
        .ok_or_else(|| {
            "headless MCP requires a process-scoped --auth-token or CATDESK_MCP_AUTH_TOKEN with at least 24 non-whitespace characters"
                .to_string()
        })?;

    if isolated_dev_fixture
        && (!web_auto_projects.is_empty()
            || host != "127.0.0.1"
            || port != 0
            || !matches!(tool_mode, ToolMode::ReadOnly)
            || !matches!(mode, Mode::Computer)
            || mcp_path.as_deref() != Some("/dev/mcp")
            || ![
                "--host",
                "--port",
                "--workspace",
                "--config-path",
                "--auth-token",
                "--mcp-path",
            ]
            .iter()
            .all(|flag| args.iter().any(|arg| arg == flag)))
    {
        return Err("isolated fixture requires explicit workspace, config, /dev/mcp, loopback port 0 and read-only computer mode".into());
    }
    Ok(Some(HeadlessMcpOptions {
        maximum_runtime_seconds,
        web_auto_projects,
        isolated_dev_fixture,
        host,
        port,
        workspace,
        mcp_path,
        mode,
        tool_mode,
        config_path,
        auth_token,
    }))
}

fn parse_auto_start_computer(args: &[String]) -> Result<bool, String> {
    let count = args
        .iter()
        .filter(|arg| arg.as_str() == "--auto-start-computer")
        .count();
    if count > 1 {
        return Err("--auto-start-computer may only be supplied once".into());
    }
    Ok(count == 1)
}

fn parse_native_daemon_mode(args: &[String]) -> Result<bool, String> {
    let count = args
        .iter()
        .filter(|arg| arg.as_str() == daemon_reload::DAEMON_MODE_FLAG)
        .count();
    if count > 1 {
        return Err(format!(
            "{} may only be supplied once",
            daemon_reload::DAEMON_MODE_FLAG
        ));
    }
    if count == 1 && args.len() != 1 {
        return Err(format!(
            "{} does not accept additional command-line arguments",
            daemon_reload::DAEMON_MODE_FLAG
        ));
    }
    Ok(count == 1)
}

fn next_arg(args: &[String], index: &mut usize, option: &str) -> Result<String, String> {
    *index += 1;
    let value = args
        .get(*index)
        .ok_or_else(|| format!("{option} requires a value"))?;
    *index += 1;
    Ok(value.clone())
}

fn validate_loopback_host(host: &str) -> Result<(), String> {
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(());
    }
    let ip = host
        .parse::<std::net::IpAddr>()
        .map_err(|_| format!("--host must be a loopback IP or localhost, got `{host}`"))?;
    if ip.is_loopback() {
        Ok(())
    } else {
        Err(format!("--host must be loopback-only, got `{host}`"))
    }
}

fn validate_headless_mcp_path(path: &str) -> Result<String, String> {
    let slug = path
        .strip_prefix('/')
        .and_then(|value| value.strip_suffix("/mcp"))
        .ok_or_else(|| "--mcp-path must match /[A-Za-z0-9_-]+/mcp".to_string())?;
    if slug.is_empty()
        || slug.contains('/')
        || slug.contains('\\')
        || !slug
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err("--mcp-path must match /[A-Za-z0-9_-]+/mcp".into());
    }
    if slug == "." || slug == ".." {
        return Err("--mcp-path slug must not be a traversal segment".into());
    }
    Ok(path.to_string())
}

fn slug_from_mcp_path(path: &str) -> Result<String, String> {
    let slug = path
        .strip_prefix('/')
        .and_then(|value| value.strip_suffix("/mcp"))
        .ok_or_else(|| "--mcp-path must match /[A-Za-z0-9_-]+/mcp".to_string())?;
    Ok(slug.to_string())
}

fn parse_headless_mode(value: &str) -> Result<Mode, String> {
    match value {
        "computer" => Ok(Mode::Computer),
        "browser" => Ok(Mode::Browser),
        "both" => Ok(Mode::Both),
        _ => Err(format!(
            "invalid --mode `{value}`; expected computer, browser, or both"
        )),
    }
}

fn parse_headless_tool_mode(value: &str) -> Result<ToolMode, String> {
    match value {
        "multi-tools" => Ok(ToolMode::MultiTools),
        "supervisor-only" => Ok(ToolMode::SupervisorOnly),
        "read-only" => Ok(ToolMode::ReadOnly),
        _ => Err(format!(
            "invalid --tool-mode `{value}`; expected multi-tools, supervisor-only, or read-only"
        )),
    }
}

fn resolve_headless_workspace(workspace: &str) -> std::io::Result<String> {
    let workspace_path = std::fs::canonicalize(workspace).map_err(|error| {
        std::io::Error::other(format!(
            "headless MCP workspace must exist and be a directory: {workspace}: {error}"
        ))
    })?;
    if !workspace_path.is_dir() {
        return Err(std::io::Error::other(format!(
            "headless MCP workspace must be a directory: {}",
            workspace_path.to_string_lossy()
        )));
    }
    Ok(workspace_path.to_string_lossy().into_owned())
}

// A developer fixture may be read by a bounded MCP endpoint, but it must
// never share the production workspace/config, even if someone supplies them
// explicitly. A fixed direct child of the OS temp root is the only scope.
fn validate_isolated_fixture_scope(
    workspace: &str,
    config: &std::path::Path,
) -> std::io::Result<()> {
    let fail = || std::io::Error::other("isolated development fixture scope rejected");
    let temp = std::fs::canonicalize(std::env::temp_dir()).map_err(|_| fail())?;
    let fixture = std::path::Path::new(workspace);
    if fixture.parent() != Some(temp.as_path())
        || !fixture
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| {
                n.starts_with("catdesk-dev-fixture-")
                    && n.len() <= 100
                    && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
        || config.file_name().and_then(|n| n.to_str()) != Some("config.toml")
        || std::fs::canonicalize(config.parent().ok_or_else(fail)?).map_err(|_| fail())? != fixture
    {
        return Err(fail());
    }
    match std::fs::symlink_metadata(config) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err(fail());
                }
                // Use the already-reviewed stable Win32 handle information path
                // to reject config hardlinks without unstable MetadataExt APIs.
                let pinned = crate::windows_protected_fs::PinnedDirectory::acquire(
                    fixture,
                    "isolated fixture directory",
                )
                .map_err(|_| fail())?;
                crate::windows_protected_fs::read_relative_regular_unique(
                    &pinned,
                    "config.toml",
                    512 * 1024,
                    "isolated fixture configuration",
                )
                .map_err(|_| fail())?;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(fail()),
    }
    Ok(())
}

async fn run_headless_mcp(options: HeadlessMcpOptions) -> Result<(), Box<dyn std::error::Error>> {
    let workspace_root = resolve_headless_workspace(&options.workspace)?;
    if catdesk_wake::development_runtime::root_for_workspace(std::path::Path::new(&workspace_root))?
        .is_some()
    {
        // Development state belongs to this explicit isolated workspace. Do
        // not load or initialize a user-supplied production configuration.
        let expected =
            std::path::Path::new(&workspace_root).join(".catdesk/web-development/catdesk.toml");
        let parent = options
            .config_path
            .parent()
            .ok_or("WEB_DEVELOPMENT_CONFIG_SCOPE_INVALID")?
            .canonicalize()?;
        if parent
            != expected
                .parent()
                .expect("fixed config parent")
                .canonicalize()?
            || options.config_path.file_name() != expected.file_name()
            || options.isolated_dev_fixture
        {
            return Err("WEB_DEVELOPMENT_CONFIG_SCOPE_INVALID".into());
        }
        match std::fs::symlink_metadata(&options.config_path) {
            Ok(metadata) => {
                #[cfg(windows)]
                let redirected = {
                    use std::os::windows::fs::MetadataExt;
                    metadata.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let redirected = metadata.file_type().is_symlink();
                if !metadata.is_file() || redirected {
                    return Err("WEB_DEVELOPMENT_CONFIG_SCOPE_INVALID".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    // Hold a no-delete-share Windows directory handle for the entire fixture
    // listener lifetime. Requests otherwise reopen the root path and could
    // follow a renamed/replaced directory after startup validation.
    let _fixture_root_guard = if options.isolated_dev_fixture {
        validate_isolated_fixture_scope(&workspace_root, &options.config_path)?;
        let pinned = windows_protected_fs::PinnedDirectory::acquire(
            std::path::Path::new(&workspace_root),
            "isolated fixture root",
        )
        .map_err(std::io::Error::other)?;
        pinned
            .assert_stable("isolated fixture root")
            .map_err(std::io::Error::other)?;
        Some(pinned)
    } else {
        None
    };
    let app = if options.isolated_dev_fixture {
        AppState::new_isolated_fixture(
            options.port,
            workspace_root.clone(),
            options.config_path.clone(),
        )?
    } else {
        AppState::new_headless(
            options.port,
            workspace_root.clone(),
            options.config_path.clone(),
        )?
    };
    let state: SharedState = Arc::new(Mutex::new(app));

    let mcp_path = {
        let mut app = state.lock().await;
        app.mode = options.mode;
        app.tool_mode = options.tool_mode;
        if let Some(path) = options.mcp_path.as_deref() {
            app.mcp_slug = slug_from_mcp_path(path).map_err(std::io::Error::other)?;
        }
        app.mcp_path()
    };

    let (ui_event_tx, mut ui_event_rx) = unbounded_channel();
    let router = if options.isolated_dev_fixture {
        server::isolated_fixture_router(
            state.clone(),
            mcp_path.clone(),
            ui_event_tx,
            options.auth_token.clone(),
        )
    } else {
        server::router(
            state.clone(),
            None,
            mcp_path.clone(),
            ui_event_tx,
            Some(options.auth_token.clone()),
        )
    };
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", options.host, options.port))
        .await
        .map_err(|error| {
            std::io::Error::other(format!(
                "failed to bind headless MCP server on {}:{}: {error}",
                options.host, options.port
            ))
        })?;
    daemon_reload::make_listener_non_inheritable(&listener).map_err(std::io::Error::other)?;
    let local_addr = listener.local_addr()?;
    if !local_addr.ip().is_loopback() {
        return Err(std::io::Error::other(format!(
            "headless MCP listener resolved to non-loopback address: {local_addr}"
        ))
        .into());
    }
    {
        let mut app = state.lock().await;
        app.port = local_addr.port();
        app.server_running = true;
        app.log(
            "INFO",
            format!("Headless MCP server started on {local_addr}{mcp_path}"),
        );
    }

    let drain_state = state.clone();
    let drain_handle = tokio::spawn(async move {
        while let Some(event) = ui_event_rx.recv().await {
            let mut app = drain_state.lock().await;
            app.apply_server_ui_event(event);
        }
    });
    let mut serve_handle = tokio::spawn(async move { axum::serve(listener, router).await });

    println!(
        "{}",
        serde_json::json!({
            "status": "ready",
            "mode": options.mode.label(),
            "tool_mode": options.tool_mode.label(),
            "workspace": workspace_root,
            "config_path": options.config_path,
            "maximum_runtime_seconds": options.maximum_runtime_seconds,
            "web_auto_projects": options.web_auto_projects,
            "mcp_url": format!("http://{local_addr}{mcp_path}"),
            "auth": {
                "scheme": "bearer",
                "source": "--auth-token|CATDESK_MCP_AUTH_TOKEN",
                "token_redacted": true
            },
        })
    );

    if !options.isolated_dev_fixture {
        if options.web_auto_projects.is_empty() {
            operator_facade::start_web_auto_scheduler(state.clone());
        } else {
            operator_facade::start_scoped_web_auto_scheduler(
                state.clone(),
                options.web_auto_projects,
            );
        }
    }

    let deadline = async {
        if let Some(seconds) = options.maximum_runtime_seconds {
            tokio::time::sleep(Duration::from_secs(seconds)).await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        _ = deadline => {}
        signal = tokio::signal::ctrl_c() => {
            signal?;
        }
        result = &mut serve_handle => {
            drain_handle.abort();
            operator_facade::stop_web_auto_scheduler(state.clone()).await;
            match result {
                Ok(Ok(())) => return Err(std::io::Error::other("headless MCP server exited unexpectedly").into()),
                Ok(Err(error)) => return Err(std::io::Error::other(format!("headless MCP server failed: {error}")).into()),
                Err(error) => return Err(std::io::Error::other(format!("headless MCP server task failed: {error}")).into()),
            }
        }
    }
    serve_handle.abort();
    drain_handle.abort();
    operator_facade::stop_web_auto_scheduler(state.clone()).await;
    Ok(())
}

fn reviewed_candidate_stage_error(
    failure: isolated_fixture_snapshot::CandidateStageError,
) -> std::io::Error {
    let reason = match failure {
        isolated_fixture_snapshot::CandidateStageError::BeforeOwnership(reason) => reason,
        isolated_fixture_snapshot::CandidateStageError::CreationIndeterminate(failure) => format!(
            "candidate root creation indeterminate for snapshot {}: {}",
            failure.manifest.snapshot_id, failure.reason
        ),
        isolated_fixture_snapshot::CandidateStageError::Owned(failure) => format!(
            "owned candidate snapshot {} requires attention: {}",
            failure.manifest.snapshot_id, failure.reason
        ),
    };
    std::io::Error::other(reason)
}

fn reviewed_candidate_runtime_error(
    snapshot: &isolated_fixture_snapshot::StagedCandidateSnapshot,
    code: &'static str,
    reason: impl std::fmt::Display,
) -> std::io::Error {
    match snapshot.mark_runtime_attention(code) {
        Ok(()) => std::io::Error::other(format!(
            "reviewed candidate {reason}; durable runtime attention recorded"
        )),
        Err(attention_error) => std::io::Error::other(format!(
            "reviewed candidate {reason}; runtime attention unavailable: {attention_error}"
        )),
    }
}

fn reviewed_candidate_startup_error(
    snapshot: &isolated_fixture_snapshot::StagedCandidateSnapshot,
    reason: impl std::fmt::Display,
) -> std::io::Error {
    match snapshot.mark_startup_attention() {
        Ok(()) => std::io::Error::other(format!(
            "reviewed candidate startup failed: {reason}; durable startup attention recorded"
        )),
        Err(attention_error) => std::io::Error::other(format!(
            "reviewed candidate startup failed: {reason}; startup attention unavailable: {attention_error}"
        )),
    }
}

/// A bound socket is not a ready MCP worker. Prove that the exact loopback
/// listener answers an authenticated request with only the isolated tools
/// before advertising its PID and port to a parent.
async fn probe_reviewed_candidate_ready(
    local_addr: std::net::SocketAddr,
    auth_token: &str,
) -> Result<(), String> {
    if !local_addr.ip().is_loopback() || local_addr.port() == 0 {
        return Err("reviewed candidate readiness address is unsafe".into());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(500))
        .build()
        .map_err(|_| "reviewed candidate readiness client unavailable")?;
    let url = format!("http://{local_addr}/dev/mcp");
    let request = serde_json::json!({
        "jsonrpc":"2.0",
        "id":"candidate-ready",
        "method":"tools/list",
        "params":{}
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match client.post(&url).json(&request).send().await {
            Ok(response) if response.status() == reqwest::StatusCode::UNAUTHORIZED => {}
            Ok(_) => return Err("reviewed candidate unauthenticated request was accepted".into()),
            Err(_) => {
                if Instant::now() >= deadline {
                    return Err("reviewed candidate readiness timed out".into());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        }
        let response = client
            .post(&url)
            .bearer_auth(auth_token)
            .json(&request)
            .send()
            .await;
        if let Ok(response) = response {
            if response.status() != reqwest::StatusCode::OK {
                return Err("reviewed candidate readiness HTTP response invalid".into());
            }
            let bytes = response
                .bytes()
                .await
                .map_err(|_| "reviewed candidate readiness response unavailable")?;
            if bytes.len() > 32 * 1024 {
                return Err("reviewed candidate readiness response oversized".into());
            }
            let reply: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|_| "reviewed candidate readiness response invalid")?;
            let tools = reply["result"]["tools"]
                .as_array()
                .ok_or("reviewed candidate readiness tool catalog unavailable")?;
            let names = tools
                .iter()
                .filter_map(|tool| tool["name"].as_str())
                .collect::<std::collections::BTreeSet<_>>();
            if reply["jsonrpc"] == "2.0"
                && reply["id"] == "candidate-ready"
                && reply.get("error").is_none()
                && tools.len() == 2
                && names == std::collections::BTreeSet::from(["read", "search"])
            {
                return Ok(());
            }
            return Err("reviewed candidate readiness tool catalog invalid".into());
        }
        if Instant::now() >= deadline {
            return Err("reviewed candidate readiness timed out".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Verify the live IPv4 listener in the Windows TCP owner table. A process
/// lock and a successful HTTP request do not establish who owns the bound
/// port; this check must pass before ready output and throughout serving.
fn verify_reviewed_candidate_listener_owner(
    local_addr: std::net::SocketAddr,
    expected_pid: u32,
) -> Result<(), String> {
    if local_addr.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        || local_addr.port() == 0
        || expected_pid == 0
    {
        return Err("reviewed candidate listener identity is unsafe".into());
    }
    #[cfg(windows)]
    {
        let owners = reviewed_candidate_ipv4_loopback_listener_owners(local_addr.port())?;
        if owners.len() != 1 || owners[0] != expected_pid {
            return Err("reviewed candidate listener PID ownership drifted".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("reviewed candidate listener ownership requires Windows".into())
    }
}

/// A live PID and bound port alone do not prove that the candidate is still
/// answering its isolated MCP route. Check the socket owner on both sides of
/// the bounded authenticated probe so a port handoff cannot pass a watch tick.
async fn verify_reviewed_candidate_service_health(
    local_addr: std::net::SocketAddr,
    auth_token: &str,
    expected_pid: u32,
) -> Result<(), String> {
    verify_reviewed_candidate_listener_owner(local_addr, expected_pid)?;
    tokio::time::timeout(
        Duration::from_secs(6),
        probe_reviewed_candidate_ready(local_addr, auth_token),
    )
    .await
    .map_err(|_| "reviewed candidate service health probe timed out")??;
    verify_reviewed_candidate_listener_owner(local_addr, expected_pid)
}

#[cfg(windows)]
fn reviewed_candidate_ipv4_loopback_listener_owners(port: u16) -> Result<Vec<u32>, String> {
    use std::ffi::c_void;

    const AF_INET: u32 = 2;
    const TCP_TABLE_OWNER_PID_LISTENER: u32 = 3;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    const MAX_TABLE_BYTES: u32 = 4 * 1024 * 1024;
    const ROW_BYTES: usize = 6 * std::mem::size_of::<u32>();

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn GetExtendedTcpTable(
            tcp_table: *mut c_void,
            size: *mut u32,
            order: i32,
            af: u32,
            table_class: u32,
            reserved: u32,
        ) -> u32;
    }

    let mut size = 0u32;
    let status = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if status != ERROR_INSUFFICIENT_BUFFER || !(4..=MAX_TABLE_BYTES).contains(&size) {
        return Err("reviewed candidate listener table size unavailable".into());
    }
    for _ in 0..3 {
        let mut table = vec![0u8; size as usize];
        let status = unsafe {
            GetExtendedTcpTable(
                table.as_mut_ptr().cast(),
                &mut size,
                0,
                AF_INET,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if status == ERROR_INSUFFICIENT_BUFFER && size <= MAX_TABLE_BYTES {
            continue;
        }
        if status != 0 || size < 4 || size as usize > table.len() {
            return Err("reviewed candidate listener table query failed".into());
        }
        let table = &table[..size as usize];
        let count = u32::from_ne_bytes(table[..4].try_into().unwrap()) as usize;
        let end = count
            .checked_mul(ROW_BYTES)
            .and_then(|bytes| bytes.checked_add(4))
            .ok_or("reviewed candidate listener table overflowed")?;
        if end > table.len() {
            return Err("reviewed candidate listener table was truncated".into());
        }
        let mut owners = Vec::new();
        for row in table[4..end].chunks_exact(ROW_BYTES) {
            let row_port = u32::from_ne_bytes(row[8..12].try_into().unwrap());
            // dwLocalAddr is network-order bytes; require the actual
            // 127.0.0.1 listener rather than another interface on this port.
            if row[4..8] == [127, 0, 0, 1] && u16::from_be((row_port & 0xffff) as u16) == port {
                owners.push(u32::from_ne_bytes(row[20..24].try_into().unwrap()));
            }
        }
        owners.sort_unstable();
        owners.dedup();
        return Ok(owners);
    }
    Err("reviewed candidate listener table kept changing".into())
}

/// One logical CatDesk reviewed-candidate owner at a time. The lock is a
/// create-once, delete-on-close Windows object beneath a pinned OS temp root;
/// a preexisting name fails closed and is never removed by pathname. Keep
/// both fields alive through root and journal retirement.
struct ReviewedCandidateProcessLock {
    _temp: windows_protected_fs::ProtectedDirectoryGuard,
    _lock: windows_protected_fs::DeleteOnCloseRegularLock,
}

fn acquire_reviewed_candidate_process_lock() -> Result<ReviewedCandidateProcessLock, String> {
    let temp = std::fs::canonicalize(std::env::temp_dir())
        .map_err(|_| "reviewed candidate temp authority unavailable")?;
    let pinned = windows_protected_fs::ProtectedDirectoryGuard::acquire(
        &temp,
        "reviewed candidate process lock",
    )?;
    let lock = windows_protected_fs::create_delete_on_close_regular_lock(
        &pinned,
        "catdesk-reviewed-candidate-owner.lock",
        "reviewed candidate process lock",
    )?;
    Ok(ReviewedCandidateProcessLock {
        _temp: pinned,
        _lock: lock,
    })
}

fn parse_owned_candidate_ready(
    line: &str,
    expected_commit: &str,
    expected_pid: u32,
) -> Result<std::net::SocketAddr, String> {
    let ready: serde_json::Value =
        serde_json::from_str(line).map_err(|_| "reviewed candidate child ready record invalid")?;
    if ready["status"] != "ready"
        || ready["kind"] != "reviewed-candidate"
        || ready["sourceCommit"] != expected_commit
        || ready["ownerPid"].as_u64() != Some(u64::from(expected_pid))
        || ready["auth"]["scheme"] != "bearer"
        || ready["auth"]["tokenRedacted"] != true
        || ready["snapshotId"].as_str().is_none_or(str::is_empty)
        || ready["runtimeGeneration"]
            .as_str()
            .is_none_or(str::is_empty)
    {
        return Err("reviewed candidate child ready identity mismatch".into());
    }
    let raw_url = ready["mcpUrl"]
        .as_str()
        .ok_or("reviewed candidate child URL unavailable")?;
    let url = reqwest::Url::parse(raw_url).map_err(|_| "reviewed candidate child URL invalid")?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.path() != "/dev/mcp"
        || url.port().is_none_or(|port| port == 0)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || raw_url != format!("http://127.0.0.1:{}/dev/mcp", url.port().unwrap_or(0))
    {
        return Err("reviewed candidate child URL is outside exact loopback scope".into());
    }
    Ok(std::net::SocketAddr::from((
        [127, 0, 0, 1],
        url.port().expect("port checked"),
    )))
}

#[cfg(windows)]
async fn fail_owned_candidate_after_health_check(
    mut owned: reviewed_candidate_owner::OwnedCandidateChild,
    reason: &str,
    wait_limit: Duration,
) -> std::io::Error {
    // This is the private parent-to-exact-child pipe created at spawn. One
    // five-byte write cannot fill its initially empty OS pipe. Closing stdin
    // afterwards also prevents a later command from changing the outcome.
    let sent = owned
        .child_mut()
        .stdin
        .as_mut()
        .ok_or_else(|| std::io::Error::other("candidate control pipe unavailable"))
        .and_then(|stdin| stdin.write_all(b"fail\n"));
    drop(owned.child_mut().stdin.take());
    let outcome = match sent {
        Ok(()) => {
            let deadline = Instant::now() + wait_limit;
            loop {
                match owned.child_mut().try_wait() {
                    Ok(Some(status)) => {
                        break format!("child exited after failure command ({status})");
                    }
                    Ok(None) if Instant::now() < deadline => {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                    Ok(None) => break "child did not exit after failure command".into(),
                    Err(_) => break "child exit status unavailable after failure command".into(),
                }
            }
        }
        Err(_) => "failure command could not be sent".into(),
    };
    // Even a child that exited after the command cannot authorize cleanup or
    // prove that its journal write succeeded. The retained exact Job/process
    // handles provide bounded termination; diagnose the journal separately.
    let cleanup = match owned.terminate_and_wait() {
        Ok(()) => "exact child tree stopped".to_owned(),
        Err(error) => format!("exact child cleanup requires attention: {error}"),
    };
    std::io::Error::other(format!(
        "reviewed candidate health failed ({reason}); {outcome}; {cleanup}; journal requires read-only diagnosis"
    ))
}

#[derive(Debug, PartialEq, Eq)]
enum ReviewedCandidateParentControl {
    Stop,
    HealthFailure,
}

fn parse_reviewed_candidate_parent_control(
    command: &[u8; 5],
) -> Result<ReviewedCandidateParentControl, String> {
    match command {
        b"stop\n" => Ok(ReviewedCandidateParentControl::Stop),
        b"fail\n" => Ok(ReviewedCandidateParentControl::HealthFailure),
        _ => Err("reviewed candidate owner control signal invalid".into()),
    }
}

#[cfg(windows)]
async fn run_reviewed_candidate_owner(
    options: ReviewedCandidateMcpOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufReader, Read};
    use std::process::{Command, Stdio};

    fn bounded_ready_line<R: Read>(reader: &mut R) -> Result<String, String> {
        let mut bytes = Vec::new();
        let mut byte = [0_u8; 1];
        for _ in 0..16_384 {
            match reader.read(&mut byte) {
                Ok(0) => return Err("reviewed candidate child exited before ready".into()),
                Ok(_) if byte[0] == b'\n' => {
                    return String::from_utf8(bytes)
                        .map_err(|_| "reviewed candidate child ready record is not UTF-8".into());
                }
                Ok(_) => bytes.push(byte[0]),
                Err(_) => return Err("reviewed candidate child ready stream failed".into()),
            }
        }
        Err("reviewed candidate child ready record oversized".into())
    }

    let executable = std::env::current_exe()?;
    let mut command = Command::new(executable);
    command
        .arg("--reviewed-candidate-mcp")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg("0")
        .arg("--mcp-path")
        .arg("/dev/mcp")
        .arg("--workspace")
        .arg(&options.source_workspace)
        .arg("--expected-commit")
        .arg(&options.expected_commit)
        .arg("--auth-token")
        .arg(&options.auth_token)
        .env("CATDESK_REVIEWED_CANDIDATE_PARENT_STDIN", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut owned = reviewed_candidate_owner::OwnedCandidateChild::spawn(&mut command)
        .map_err(std::io::Error::other)?;
    let pid = owned.pid();
    let stdout = owned
        .child_mut()
        .stdout
        .take()
        .ok_or("reviewed candidate child stdout unavailable")?;
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("candidate-ready-reader".into())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            let _ = ready_tx.send(bounded_ready_line(&mut reader));
            // Continue draining bounded-process output so a future diagnostic
            // line cannot block the supervised child on a full pipe.
            let _ = std::io::copy(&mut reader, &mut std::io::sink());
        })?;
    let ready_line = tokio::time::timeout(Duration::from_secs(120), ready_rx)
        .await
        .map_err(|_| "reviewed candidate child readiness timed out")?
        .map_err(|_| "reviewed candidate child ready reader failed")?
        .map_err(std::io::Error::other)?;
    let address = parse_owned_candidate_ready(&ready_line, &options.expected_commit, pid)
        .map_err(std::io::Error::other)?;
    if owned.child_mut().try_wait()?.is_some() {
        return Err(
            std::io::Error::other("reviewed candidate child exited before owner ready").into(),
        );
    }
    if let Err(error) =
        verify_reviewed_candidate_service_health(address, &options.auth_token, pid).await
    {
        return Err(fail_owned_candidate_after_health_check(
            owned,
            &error,
            Duration::from_secs(10),
        )
        .await
        .into());
    }
    if owned.child_mut().try_wait()?.is_some() {
        return Err(
            std::io::Error::other("reviewed candidate child exited during owner probe").into(),
        );
    }
    println!("{ready_line}");

    let mut watch = tokio::time::interval(Duration::from_secs(1));
    watch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // The one-second owner check is cheap. Run the two-request MCP probe less
    // often so ordinary read/search requests are not continuously contended.
    // Its six-second deadline bounds detection of a wedged but still-bound
    // listener to about one interval plus one probe.
    let mut service_watch = tokio::time::interval(Duration::from_secs(15));
    service_watch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    service_watch.tick().await; // initial readiness probe already succeeded
    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|_| "reviewed candidate owner signal failed")?;
                break;
            }
            _ = watch.tick() => {
                if let Some(status) = owned.child_mut().try_wait()? {
                    return Err(std::io::Error::other(format!(
                        "reviewed candidate child exited unexpectedly ({status}); journal requires attention"
                    )).into());
                }
                if let Err(error) = verify_reviewed_candidate_listener_owner(address, pid) {
                    return Err(fail_owned_candidate_after_health_check(
                        owned,
                        &error,
                        Duration::from_secs(10),
                    ).await.into());
                }
            }
            _ = service_watch.tick() => {
                if let Err(error) =
                    verify_reviewed_candidate_service_health(address, &options.auth_token, pid).await
                {
                    return Err(fail_owned_candidate_after_health_check(
                        owned,
                        &error,
                        Duration::from_secs(10),
                    ).await.into());
                }
            }
        }
    }

    if let Some(status) = owned.child_mut().try_wait()? {
        if status.success() {
            return Ok(());
        }
        return Err(std::io::Error::other(format!(
            "reviewed candidate child exited during stop ({status}); journal requires attention"
        ))
        .into());
    }
    let stop_sent = {
        let stdin = owned
            .child_mut()
            .stdin
            .as_mut()
            .ok_or("reviewed candidate owner control channel unavailable")?;
        stdin.write_all(b"stop\n").and_then(|()| stdin.flush())
    };
    if let Err(error) = stop_sent {
        if owned
            .child_mut()
            .try_wait()?
            .is_some_and(|status| status.success())
        {
            return Ok(());
        }
        return Err(std::io::Error::other(format!(
            "reviewed candidate owner stop signal failed ({error}); journal requires attention"
        ))
        .into());
    }
    drop(owned.child_mut().stdin.take());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        match owned.child_mut().try_wait()? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(std::io::Error::other(format!(
                    "reviewed candidate child shutdown failed ({status}); journal requires attention"
                )).into());
            }
            None if std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            None => {
                owned.terminate_and_wait().map_err(std::io::Error::other)?;
                return Err(std::io::Error::other(
                    "reviewed candidate child shutdown timed out; journal requires attention",
                )
                .into());
            }
        }
    }
}

#[cfg(not(windows))]
async fn run_reviewed_candidate_owner(
    _options: ReviewedCandidateMcpOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    Err(std::io::Error::other("reviewed candidate owner requires Windows").into())
}

async fn wait_reviewed_candidate_parent_control() -> Result<ReviewedCandidateParentControl, String>
{
    use tokio::io::AsyncReadExt;
    let mut stop = [0_u8; 5];
    tokio::io::stdin()
        .read_exact(&mut stop)
        .await
        .map_err(|_| "reviewed candidate owner control channel closed")?;
    parse_reviewed_candidate_parent_control(&stop)
}

#[cfg(windows)]
fn candidate_owner_recovery_class_name(
    class: isolated_fixture_snapshot::CandidateOwnerRecoveryClass,
) -> &'static str {
    use isolated_fixture_snapshot::CandidateOwnerRecoveryClass;
    match class {
        CandidateOwnerRecoveryClass::NoJournal => "NO_JOURNAL",
        CandidateOwnerRecoveryClass::IntentBeforeRoot => "INTENT_BEFORE_ROOT",
        CandidateOwnerRecoveryClass::BoundBeforeServing => "BOUND_BEFORE_SERVING",
        CandidateOwnerRecoveryClass::OwnerProcessAlive => "OWNER_PROCESS_ALIVE",
        CandidateOwnerRecoveryClass::OrphanedOwnerExited => "ORPHANED_OWNER_EXITED",
        CandidateOwnerRecoveryClass::OrphanedPidReused => "ORPHANED_PID_REUSED",
        CandidateOwnerRecoveryClass::RetiredJournalPresent => "RETIRED_JOURNAL_PRESENT",
        CandidateOwnerRecoveryClass::Attention => "ATTENTION",
    }
}

#[cfg(windows)]
fn format_candidate_owner_diagnostic_report(
    report: isolated_fixture_snapshot::CandidateOwnerRecoveryReport,
    source_commit: &str,
    witness_run_id: u64,
    witness_run_attempt: u64,
    temp_parent: &str,
) -> Result<String, String> {
    serde_json::to_string(&serde_json::json!({
        "snapshotId": report.snapshot_id,
        "sourceCommit": source_commit,
        "witnessRunId": witness_run_id,
        "witnessRunAttempt": witness_run_attempt,
        "witnessScope": "HISTORICAL_COMMIT_PUSH_CI",
        "scope": "EXACT_SNAPSHOT_IN_DIAGNOSTIC_PROCESS_TEMP_PARENT",
        "tempParent": temp_parent,
        "authority": "DIAGNOSTIC_ONLY",
        "classification": candidate_owner_recovery_class_name(report.class),
        "detail": report.detail,
    }))
    .map_err(|_| "candidate owner diagnostic report encoding failed".into())
}

#[cfg(windows)]
async fn run_reviewed_candidate_owner_diagnose(
    options: ReviewedCandidateOwnerDiagnoseOptions,
) -> Result<String, String> {
    let source = std::fs::canonicalize(&options.source_workspace)
        .map_err(|_| "candidate owner diagnostic source workspace unavailable")?;
    if !source.is_dir() {
        return Err("candidate owner diagnostic source workspace invalid".into());
    }
    // Reconstruct the exact historical manifest from a fixed-branch push CI
    // diagnostic witness and the two immutable Git blobs. The distinct witness
    // cannot satisfy the current-head fixture launch gate. No caller supplies
    // a manifest, journal pathname, or candidate root pathname.
    let witness = isolated_fixture_github_authority::fetch_historical_diagnostic_witness(
        &options.expected_commit,
    )
    .await
    .map_err(|error| format!("historical commit push CI required: {error}"))?;
    let manifest = isolated_fixture_git_objects::read_historical_diagnostic_git_objects(
        &source,
        &witness,
        &options.snapshot_id,
    )
    .await?;
    if manifest.source_commit != witness.commit_sha() || manifest.snapshot_id != options.snapshot_id
    {
        return Err("candidate owner diagnostic witnessed manifest drifted".into());
    }
    let temp_parent = std::fs::canonicalize(std::env::temp_dir())
        .map_err(|_| "candidate owner diagnostic temp parent unavailable")?;
    if !temp_parent.is_dir() {
        return Err("candidate owner diagnostic temp parent invalid".into());
    }
    let temp_parent_text = temp_parent
        .to_str()
        .ok_or("candidate owner diagnostic temp parent is not UTF-8")?;
    let report = isolated_fixture_snapshot::classify_candidate_owner_recovery(&manifest);
    if std::fs::canonicalize(std::env::temp_dir())
        .map_err(|_| "candidate owner diagnostic temp parent unavailable")?
        != temp_parent
    {
        return Err("candidate owner diagnostic temp parent changed".into());
    }
    format_candidate_owner_diagnostic_report(
        report,
        witness.commit_sha(),
        witness.run_id(),
        witness.run_attempt(),
        temp_parent_text,
    )
}

#[cfg(not(windows))]
async fn run_reviewed_candidate_owner_diagnose(
    _options: ReviewedCandidateOwnerDiagnoseOptions,
) -> Result<String, String> {
    Err("candidate owner diagnosis requires Windows".into())
}

async fn run_reviewed_candidate_mcp(
    options: ReviewedCandidateMcpOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let _owner_lock = acquire_reviewed_candidate_process_lock().map_err(std::io::Error::other)?;
    let parent_stdin_shutdown =
        std::env::var_os("CATDESK_REVIEWED_CANDIDATE_PARENT_STDIN").is_some();
    let source = std::fs::canonicalize(&options.source_workspace)?;
    if !source.is_dir() {
        return Err(std::io::Error::other("reviewed candidate source workspace invalid").into());
    }
    // The witness and fixed Git-object reader run before any app state or
    // listener exists. A missing exact three-job CI proof fails closed here.
    let staged = isolated_fixture_snapshot::stage_witnessed_candidate_snapshot(
        &source,
        &options.expected_commit,
    )
    .await
    .map_err(reviewed_candidate_stage_error)?;
    let fixture_root = staged.root.path().to_path_buf();
    let snapshot_id = staged.manifest.snapshot_id.clone();
    let source_commit = staged.manifest.source_commit.clone();
    let app = AppState::new_isolated_fixture(
        0,
        fixture_root.to_string_lossy().into_owned(),
        fixture_root.join("config.toml"),
    )
    .map_err(|error| reviewed_candidate_startup_error(&staged, error))?;
    let state: SharedState = Arc::new(Mutex::new(app));
    {
        let mut app = state.lock().await;
        app.mode = Mode::Computer;
        app.tool_mode = ToolMode::ReadOnly;
        app.mcp_slug = "dev".into();
    }
    let (ui_event_tx, _ui_event_rx) = unbounded_channel();
    let snapshot = Arc::new(staged);
    let readiness_token = options.auth_token.clone();
    let router = server::isolated_fixture_snapshot_router(
        state.clone(),
        "/dev/mcp".into(),
        ui_event_tx,
        options.auth_token,
        snapshot.clone(),
    )
    .await
    .map_err(|error| reviewed_candidate_startup_error(&snapshot, error))?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| reviewed_candidate_startup_error(&snapshot, error))?;
    daemon_reload::make_listener_non_inheritable(&listener)
        .map_err(|error| reviewed_candidate_startup_error(&snapshot, error))?;
    let local_addr = listener
        .local_addr()
        .map_err(|error| reviewed_candidate_startup_error(&snapshot, error))?;
    if !local_addr.ip().is_loopback() {
        return Err(reviewed_candidate_startup_error(&snapshot, "listener is not loopback").into());
    }
    let runtime_generation = snapshot
        .mark_serving(local_addr.port())
        .map_err(|error| reviewed_candidate_startup_error(&snapshot, error))?;
    {
        let mut app = state.lock().await;
        app.port = local_addr.port();
        app.server_running = true;
    }
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let mut serve = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });
    let readiness = tokio::select! {
        result = &mut serve => Err(format!("reviewed candidate listener exited before ready: {result:?}")),
        result = probe_reviewed_candidate_ready(local_addr, &readiness_token) => result,
    };
    if let Err(error) = readiness {
        if !serve.is_finished() {
            serve.abort();
            let _ = tokio::time::timeout(Duration::from_secs(2), &mut serve).await;
        }
        return Err(reviewed_candidate_startup_error(&snapshot, error).into());
    }
    if let Err(error) = verify_reviewed_candidate_listener_owner(local_addr, std::process::id()) {
        serve.abort();
        let _ = tokio::time::timeout(Duration::from_secs(2), &mut serve).await;
        return Err(reviewed_candidate_startup_error(&snapshot, error).into());
    }
    if serve.is_finished() {
        return Err(
            reviewed_candidate_startup_error(&snapshot, "listener exited before ready").into(),
        );
    }
    println!(
        "{}",
        serde_json::json!({
            "status":"ready",
            "kind":"reviewed-candidate",
            "snapshotId":snapshot_id,
            "sourceCommit":source_commit,
            "runtimeGeneration":runtime_generation,
            "ownerPid":std::process::id(),
            "mcpUrl":format!("http://{local_addr}/dev/mcp"),
            "auth":{"scheme":"bearer","tokenRedacted":true}
        })
    );
    let parent_control = wait_reviewed_candidate_parent_control();
    tokio::pin!(parent_control);
    let mut owner_watch = tokio::time::interval(Duration::from_secs(1));
    owner_watch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            parent_control_result = &mut parent_control, if parent_stdin_shutdown => {
                match parent_control_result {
                    Ok(ReviewedCandidateParentControl::Stop) => break,
                    Ok(ReviewedCandidateParentControl::HealthFailure) => {
                        serve.abort();
                        let _ = tokio::time::timeout(Duration::from_secs(2), &mut serve).await;
                        return Err(reviewed_candidate_runtime_error(
                            &snapshot,
                            "owner_health_failed",
                            "parent health check failed",
                        ).into());
                    }
                    Err(error) => {
                        serve.abort();
                        let _ = tokio::time::timeout(Duration::from_secs(2), &mut serve).await;
                        return Err(reviewed_candidate_runtime_error(&snapshot, "parent_channel_failed", error).into());
                    }
                }
            }
            signal = tokio::signal::ctrl_c() => {
                if let Err(error) = signal {
                    serve.abort();
                    let _ = tokio::time::timeout(Duration::from_secs(2), &mut serve).await;
                    return Err(reviewed_candidate_runtime_error(&snapshot, "signal_failed", error).into());
                }
                break;
            },
            result = &mut serve => {
                return Err(reviewed_candidate_runtime_error(
                    &snapshot,
                    "listener_exited",
                    format!("listener exited unexpectedly: {result:?}"),
                ).into());
            },
            _ = owner_watch.tick() => {
                if let Err(error) = verify_reviewed_candidate_listener_owner(local_addr, std::process::id()) {
                    serve.abort();
                    let _ = tokio::time::timeout(Duration::from_secs(2), &mut serve).await;
                    return Err(reviewed_candidate_runtime_error(&snapshot, "listener_exited", error).into());
                }
            }
        }
    }
    let _ = shutdown_tx.send(());
    match tokio::time::timeout(Duration::from_secs(10), &mut serve).await {
        Ok(Ok(Ok(()))) => {}
        other => {
            serve.abort();
            let _ = tokio::time::timeout(Duration::from_secs(2), &mut serve).await;
            return Err(reviewed_candidate_runtime_error(
                &snapshot,
                "shutdown_failed",
                format!("shutdown requires attention: {other:?}"),
            )
            .into());
        }
    }
    operator_facade::stop_web_auto_scheduler(state.clone()).await;
    let staged = match Arc::try_unwrap(snapshot) {
        Ok(staged) => staged,
        Err(snapshot) => {
            return Err(reviewed_candidate_runtime_error(
                &snapshot,
                "source_leases_active",
                "still has active source leases",
            )
            .into());
        }
    };
    staged.retire_owned_root().map_err(std::io::Error::other)?;
    Ok(())
}

async fn run_native_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let port = crate::control_plane_supervisor::FIXED_LOCAL_BACKEND_PORT;
    let workspace_root = match std::env::var("WORKSPACE_ROOT") {
        Ok(path) => path,
        Err(_) => std::env::current_dir()?.to_string_lossy().into_owned(),
    };
    binagotchy_cli::launch_associated(std::path::Path::new(&workspace_root));
    let state: SharedState = Arc::new(Mutex::new(AppState::new(port, workspace_root)?));
    {
        let mut app = state.lock().await;
        if app.port != crate::control_plane_supervisor::FIXED_LOCAL_BACKEND_PORT
            || app.mcp_bind_host != crate::control_plane_supervisor::FIXED_SUPERVISOR_LOOPBACK_HOST
        {
            return Err(std::io::Error::other(
                "native CatDesk daemon refuses non-canonical supervisor backend listener",
            )
            .into());
        }
        app.mode = Mode::Computer;
        app.log("INFO", "Native headless daemon mode".into());
        app.persist_state_with_log();
    }

    let (ui_event_tx, mut ui_event_rx) = unbounded_channel();
    let _devtools = start_services(state.clone(), ui_event_tx).await;
    if !state.lock().await.server_running {
        return Err(
            std::io::Error::other("native CatDesk daemon failed to start local MCP").into(),
        );
    }

    let drain_state = state.clone();
    tokio::spawn(async move {
        while let Some(event) = ui_event_rx.recv().await {
            let mut app = drain_state.lock().await;
            app.apply_server_ui_event(event);
        }
    });

    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        if !state.lock().await.server_running {
            return Err(std::io::Error::other("native CatDesk daemon local MCP stopped").into());
        }
    }
}

// ── Main ────────────────────────────────────────────────────

fn operator_web_error_code(error: &str) -> &str {
    const REFUSED: &str = "WEB_OPERATION_REFUSED";
    let code = error.split_once(':').map_or(error, |(code, _)| code);
    if code.len() < 5
        || code.len() > 64
        || !code.contains('_')
        || !code
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_uppercase())
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        || ![
            "WEB_",
            "DEV_",
            "WAKE_",
            "STATE_",
            "CONFIG_",
            "TARGET_",
            "SOURCE_",
            "QUEUE_",
            "BOOTSTRAP_",
            "BROWSER_",
            "REGISTRY_",
            "RESPONSE_",
            "LOGIN_",
            "COOPERATIVE_",
            "HOST_",
        ]
        .iter()
        .any(|prefix| code.starts_with(prefix))
    {
        return REFUSED;
    }
    code
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let operator_codex = args.first().is_some_and(|arg| arg == "codex")
        || (args.first().is_some_and(|arg| arg == "operator")
            && args.get(1).is_some_and(|arg| arg == "codex"));
    let operator_web_development = args.first().is_some_and(|arg| arg == "operator")
        && args.get(1).is_some_and(|arg| arg == "web-dev-runtime");
    let operator_web = args.first().is_some_and(|arg| arg == "operator")
        && args.get(1).is_some_and(|arg| arg == "web");
    match operator_facade::parse_operator_action(args.clone()) {
        Ok(Some(action)) => {
            let workspace = std::env::current_dir()?;
            match operator_facade::execute_operator_action(action, &workspace).await {
                Ok(result) => {
                    println!("{result}");
                    return Ok(());
                }
                Err(error) => {
                    if operator_codex || operator_web_development {
                        println!(
                            "{}",
                            serde_json::json!({"status":"attention","error":error})
                        );
                    } else if operator_web {
                        println!(
                            "{}",
                            serde_json::json!({"status":"attention","errorCode":operator_web_error_code(&error)})
                        );
                    } else {
                        println!("{{\"status\":\"attention\"}}");
                    }
                    std::process::exit(2);
                }
            }
        }
        Ok(None) => {}
        Err(error) => {
            if operator_codex || operator_web_development {
                println!(
                    "{}",
                    serde_json::json!({"status":"attention","error":error})
                );
            } else if operator_web {
                println!(
                    "{}",
                    serde_json::json!({"status":"attention","errorCode":operator_web_error_code(&error)})
                );
            } else {
                println!("{{\"status\":\"attention\"}}");
            }
            std::process::exit(2);
        }
    }
    if !args.iter().any(|arg| arg == "--headless-mcp")
        && catdesk_wake::development_runtime::root_for_workspace(&std::env::current_dir()?)?
            .is_some()
    {
        return Err("WEB_DEVELOPMENT_USE_OPERATOR_OR_HEADLESS_MODE".into());
    }
    match binagotchy_cli::parse_binagotchy_cli_mode(&args) {
        Ok(true) => {
            let workspace = std::env::current_dir()?;
            if let Err(error) = binagotchy_cli::run_cli(&workspace) {
                eprintln!("CatDesk Binagotchy: {error}");
                std::process::exit(2);
            }
            return Ok(());
        }
        Ok(false) => {}
        Err(error) => {
            eprintln!("CatDesk Binagotchy: {error}");
            std::process::exit(2);
        }
    }
    // Retain the native Win32 surface only as explicit legacy/debug
    // compatibility. Normal CatDesk launches use the console companion above.
    match windows_gui::parse_binagotchy_gui_mode(&args) {
        Ok(true) => {
            let workspace = std::env::current_dir()?;
            if let Err(error) = windows_gui::run_gui(&workspace) {
                eprintln!("CatDesk Binagotchy legacy GUI: {error}");
                std::process::exit(2);
            }
            return Ok(());
        }
        Ok(false) => {}
        Err(error) => {
            eprintln!("CatDesk Binagotchy legacy GUI: {error}");
            std::process::exit(2);
        }
    }
    match reviewed_build::parse_dedicated_producer_service_args(&args) {
        Ok(true) => match reviewed_build::run_dedicated_producer_service_command() {
            Ok(()) => return Ok(()),
            Err(error) => {
                eprintln!("CatDesk dedicated producer service: {error}");
                std::process::exit(2);
            }
        },
        Ok(false) => {}
        Err(error) => {
            eprintln!("CatDesk dedicated producer service: {error}");
            std::process::exit(2);
        }
    }
    match reviewed_build::parse_reviewed_main_image_status_args(&args) {
        Ok(true) => match reviewed_build::run_reviewed_main_image_status_command() {
            Ok(status) => {
                println!("{status}");
                return Ok(());
            }
            Err(error) => {
                eprintln!("CatDesk signed main-image status: {error}");
                std::process::exit(2);
            }
        },
        Ok(false) => {}
        Err(error) => {
            eprintln!("CatDesk signed main-image status: {error}");
            std::process::exit(2);
        }
    }
    match reviewed_build::parse_reviewed_main_image_rotate_args(&args) {
        Ok(true) => match reviewed_build::run_reviewed_main_image_rotate_command() {
            Ok(result) => {
                println!("{result}");
                return Ok(());
            }
            Err(error) => {
                eprintln!("CatDesk reviewed main-image rotation: {error}");
                std::process::exit(2);
            }
        },
        Ok(false) => {}
        Err(error) => {
            eprintln!("CatDesk reviewed main-image rotation: {error}");
            std::process::exit(2);
        }
    }
    match reviewed_build::parse_reviewed_main_image_bootstrap_install_args(&args) {
        Ok(true) => match reviewed_build::run_reviewed_main_image_bootstrap_install_command() {
            Ok(result) => {
                println!("{result}");
                return Ok(());
            }
            Err(error) => {
                eprintln!("CatDesk reviewed main-image bootstrap: {error}");
                std::process::exit(2);
            }
        },
        Ok(false) => {}
        Err(error) => {
            eprintln!("CatDesk reviewed main-image bootstrap: {error}");
            std::process::exit(2);
        }
    }
    match reviewed_build::parse_dedicated_producer_admin_provision_args(&args) {
        Ok(true) => match reviewed_build::run_dedicated_producer_admin_provision_command() {
            Ok(result) => {
                println!("{result}");
                return Ok(());
            }
            Err(error) => {
                eprintln!("CatDesk dedicated producer provisioning: {error}");
                std::process::exit(2);
            }
        },
        Ok(false) => {}
        Err(error) => {
            eprintln!("CatDesk dedicated producer provisioning: {error}");
            std::process::exit(2);
        }
    }
    match daemon_reload::parse_lifecycle_stop_worker_args(&args) {
        Ok(Some(worker)) => {
            if let Err(error) = daemon_reload::run_lifecycle_stop_worker(worker) {
                eprintln!("CatDesk lifecycle stop helper: {error}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("CatDesk lifecycle stop helper: {error}");
            std::process::exit(2);
        }
    }
    match daemon_reload::parse_canonical_recovery_worker_args(&args) {
        Ok(Some(worker)) => {
            if let Err(error) = daemon_reload::run_canonical_recovery_worker(worker) {
                eprintln!("CatDesk canonical recovery helper: {error}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("CatDesk canonical recovery helper: {error}");
            std::process::exit(2);
        }
    }
    match daemon_reload::parse_reviewed_promotion_worker_args(&args) {
        Ok(Some(worker)) => {
            if let Err(error) = daemon_reload::run_reviewed_promotion_worker(worker) {
                eprintln!("CatDesk reviewed promotion helper: {error}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("CatDesk reviewed promotion helper: {error}");
            std::process::exit(2);
        }
    }
    match daemon_reload::parse_release_pair_reconcile_worker_args(&args) {
        Ok(Some(worker)) => {
            if let Err(error) = daemon_reload::run_release_pair_reconcile_worker(worker) {
                eprintln!("CatDesk release-pair reconciliation helper: {error}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("CatDesk release-pair reconciliation helper: {error}");
            std::process::exit(2);
        }
    }
    match reviewed_build::parse_reviewed_build_worker_args(&args) {
        Ok(Some((workspace, attempt, owner))) => {
            if let Err(error) =
                reviewed_build::run_reviewed_build_worker(&workspace, &attempt, &owner)
            {
                eprintln!("CatDesk reviewed build helper: {error}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("CatDesk reviewed build helper: {error}");
            std::process::exit(2);
        }
    }
    match daemon_reload::parse_reload_worker_args(&args) {
        Ok(Some(worker)) => {
            if let Err(error) = daemon_reload::run_reload_worker(worker) {
                eprintln!("CatDesk reload helper: {error}");
                std::process::exit(1);
            }
            return Ok(());
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("CatDesk reload helper: {error}");
            std::process::exit(2);
        }
    }
    match control_plane_supervisor::parse_fixed_stable_supervisor_runtime_args(&args) {
        Ok(true) => {
            if let Err(reason) =
                control_plane_supervisor::run_fixed_stable_supervisor_runtime().await
            {
                eprintln!("CatDesk stable supervisor: {}", reason.as_str());
                std::process::exit(1);
            }
            return Ok(());
        }
        Ok(false) => {}
        Err(error) => {
            eprintln!("CatDesk: {error}");
            std::process::exit(2);
        }
    }
    match parse_native_daemon_mode(&args) {
        Ok(true) => return run_native_daemon().await,
        Ok(false) => {}
        Err(error) => {
            eprintln!("CatDesk: {error}");
            std::process::exit(2);
        }
    }
    match parse_reviewed_candidate_owner_diagnose_options(&args) {
        Ok(Some(options)) => match run_reviewed_candidate_owner_diagnose(options).await {
            Ok(report) => {
                println!("{report}");
                return Ok(());
            }
            Err(error) => {
                eprintln!("CatDesk candidate owner diagnosis: {error}");
                std::process::exit(2);
            }
        },
        Ok(None) => {}
        Err(error) => {
            eprintln!("CatDesk candidate owner diagnosis: {error}");
            std::process::exit(2);
        }
    }
    match parse_reviewed_candidate_owner_options(&args) {
        Ok(Some(options)) => return run_reviewed_candidate_owner(options).await,
        Ok(None) => {}
        Err(error) => {
            eprintln!("CatDesk: {error}");
            std::process::exit(2);
        }
    }
    match parse_reviewed_candidate_mcp_options(args.clone()) {
        Ok(Some(options)) => return run_reviewed_candidate_mcp(options).await,
        Ok(None) => {}
        Err(error) => {
            eprintln!("CatDesk: {error}");
            std::process::exit(2);
        }
    }
    match parse_headless_mcp_options(args.clone()) {
        Ok(Some(options)) => return run_headless_mcp(options).await,
        Ok(None) => {}
        Err(error) => {
            eprintln!("CatDesk: {error}");
            std::process::exit(2);
        }
    }

    // On Windows, a normal interactive launch is presentation-only. The
    // canonical daemon is owned by the lifecycle/recovery path and Binagotchy
    // is a separate singleton companion, so a bare catdesk.exe must never
    // construct a second AppState and race the daemon for 127.0.0.1:3200.
    // Explicit daemon/headless modes above remain unchanged; legacy TUI use is
    // still available through its explicit arguments rather than double-click
    // or a bare command invocation.
    #[cfg(target_os = "windows")]
    if args.is_empty() {
        let workspace_root = match std::env::var("WORKSPACE_ROOT") {
            Ok(path) => path,
            Err(_) => std::env::current_dir()?.to_string_lossy().into_owned(),
        };
        binagotchy_cli::launch_associated(std::path::Path::new(&workspace_root));
        return Ok(());
    }

    let auto_start_computer = match parse_auto_start_computer(&args) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("CatDesk: {error}");
            std::process::exit(2);
        }
    };

    match macos_terminal::maybe_relaunch_in_terminal_profile() {
        Ok(macos_terminal::LaunchAction::Continue) => {}
        #[cfg(target_os = "macos")]
        Ok(macos_terminal::LaunchAction::ExitAfterProfileBootstrap) => {
            eprintln!(
                "CatDesk applied the Terminal.app profile. Run the same command again in this tab."
            );
            return Ok(());
        }
        Err(error) => {
            return Err(std::io::Error::other(format!(
                "CatDesk: macOS Terminal profile bootstrap failed: {error}"
            ))
            .into());
        }
    }

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3200);
    let workspace_root = match std::env::var("WORKSPACE_ROOT") {
        Ok(path) => path,
        Err(_) => std::env::current_dir()?.to_string_lossy().into_owned(),
    };

    binagotchy_cli::launch_associated(std::path::Path::new(&workspace_root));
    let state: SharedState = Arc::new(Mutex::new(AppState::new(port, workspace_root)?));

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableBracketedPaste)?;
    stdout().execute(EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, state.clone(), auto_start_computer).await;

    stdout().execute(DisableBracketedPaste)?;
    stdout().execute(DisableMouseCapture)?;
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    // Cleanup after the TUI is gone so quit never appears frozen on screen.
    {
        let mut app = state.lock().await;
        if let Some(handle) = app.server_handle.take() {
            handle.abort();
        }
        if let Some(handle) = app.ngrok_task.take() {
            handle.abort();
        }
        if let Some(handle) = app.openai_tunnel_monitor_task.take() {
            handle.abort();
        }
        if let Some(child) = app.remote_browser_child.as_mut() {
            let _ = child.start_kill();
        }
        if let Some(child) = app.devtools_child.as_mut() {
            let _ = child.start_kill();
        }
        let openai_tunnel_child = app.openai_tunnel_child.take();
        app.server_running = false;
        let web_auto_tasks = app.cancel_web_auto_scheduler();
        app.ngrok_running = false;
        app.ngrok_url = None;
        app.remote_connected = false;
        app.last_remote_activity_ms = None;
        drop(app);
        operator_facade::join_web_auto_schedulers(web_auto_tasks).await;
        if let Some(mut child) = openai_tunnel_child {
            let _ = child.start_kill();
            let _ = child.wait_with_timeout(Duration::from_secs(3)).await;
        }
    }

    result
}

// ── Phase 1: Mode selection ─────────────────────────────────

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: SharedState,
    auto_start_computer: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mode = if auto_start_computer {
        Mode::Computer
    } else {
        // Draw mode selection screen.
        loop {
            let (current_theme, current_tool_mode) = {
                let app = state.lock().await;
                (app.current_theme(), app.tool_mode)
            };
            terminal.draw(|f| draw_mode_select(f, current_theme, current_tool_mode))?;

            if event::poll(UI_POLL_INTERVAL)? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    let mode = match key.code {
                        KeyCode::Char('1') => Mode::Computer,
                        KeyCode::Char('2') => Mode::Browser,
                        KeyCode::Char('3') => Mode::Both,
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('s') => {
                            run_settings(terminal, state.clone()).await?;
                            continue;
                        }
                        _ => continue,
                    };
                    break mode;
                }
            }
        }
    };

    {
        let mut app = state.lock().await;
        app.mode = mode;
        app.log("INFO", format!("Mode: {}", mode.label()));
        app.persist_state_with_log();
    }

    if mode_is_browser_enabled(state.clone()).await {
        let continue_run = run_browser_select(terminal, state.clone()).await?;
        if !continue_run {
            return Ok(());
        }
    }

    let continue_run = run_ngrok_auth_setup(terminal, state.clone()).await?;
    if !continue_run {
        return Ok(());
    }

    // Start services
    let (ui_event_tx, ui_event_rx) = unbounded_channel();
    let devtools_bridge = start_services(state.clone(), ui_event_tx).await;

    // Phase 2: main TUI loop
    run_tui(terminal, state, devtools_bridge, ui_event_rx).await
}

fn draw_mode_select(f: &mut Frame, theme: &theme::ThemeDef, tool_mode: ToolMode) {
    let palette = theme.palette;
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Length(16), // Mode selection
            Constraint::Min(0),     // Spacer
        ])
        .split(area);

    let header = Paragraph::new("  CatDesk - Turns ChatGPT Web into a coding agent =w=")
        .style(
            Style::default()
                .fg(palette.header_fg)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(palette.border_type)
                .border_style(Style::default().fg(palette.border_fg)),
        );
    f.render_widget(header, chunks[0]);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Select mode:",
            Style::default()
                .fg(palette.title_fg)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [1] ",
                Style::default()
                    .fg(palette.key_fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Control Computer   ",
                Style::default().fg(palette.primary_fg),
            ),
            Span::styled("(local tools)", Style::default().fg(palette.muted_fg)),
        ]),
        Line::from(vec![
            Span::styled(
                "  [2] ",
                Style::default()
                    .fg(palette.key_fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Control Browser    ",
                Style::default().fg(palette.primary_fg),
            ),
            Span::styled(
                "(chrome-devtools-mcp)",
                Style::default().fg(palette.muted_fg),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  [3] ",
                Style::default()
                    .fg(palette.key_fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Both", Style::default().fg(palette.primary_fg)),
        ]),
        Line::from(vec![
            Span::styled(
                "  [s] ",
                Style::default()
                    .fg(palette.key_fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Settings", Style::default().fg(palette.primary_fg)),
            Span::styled(
                format!(
                    " (theme: {}, tool mode: {})",
                    theme.label,
                    tool_mode.label()
                ),
                Style::default().fg(palette.muted_fg),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [q] ", Style::default().fg(palette.danger_fg)),
            Span::styled("Quit", Style::default().fg(palette.muted_fg)),
        ]),
    ];

    let select = Paragraph::new(lines).block(
        Block::default()
            .title(" Mode ")
            .borders(Borders::ALL)
            .border_type(palette.border_type)
            .border_style(Style::default().fg(palette.border_fg)),
    );
    f.render_widget(select, chunks[1]);
}

async fn run_ngrok_auth_setup(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: SharedState,
) -> Result<bool, Box<dyn std::error::Error>> {
    if load_ngrok_authtoken()?.is_some() {
        return Ok(true);
    }

    let config_path = app_config_path()?;
    let config_path_text = config_path.to_string_lossy().into_owned();
    let mut input = String::new();
    let mut error_message: Option<String> = None;
    let mut toast: Option<(&str, (u16, u16), Instant)> = None;

    loop {
        if let Some((_, _, t)) = &toast {
            if t.elapsed().as_secs() >= 2 {
                toast = None;
            }
        }

        let (current_theme, current_tool_mode, current_mode, browsers, selected_browser) = {
            let app = state.lock().await;
            (
                app.current_theme(),
                app.tool_mode,
                app.mode,
                app.detected_browsers.clone(),
                app.selected_browser.clone(),
            )
        };
        let supported_indices: Vec<usize> = browsers
            .iter()
            .enumerate()
            .filter(|(_, browser)| browser.mcp_supported)
            .map(|(idx, _)| idx)
            .collect();
        let selected_supported_idx =
            selected_supported_browser_idx(&browsers, selected_browser.as_ref());
        let toast_ref = toast
            .as_ref()
            .filter(|(_, _, t)| t.elapsed().as_secs() < 2)
            .map(|(m, pos, _)| (*m, *pos));
        let mut ngrok_setup_copy_area = Rect::default();
        terminal.draw(|f| {
            let anchor_area = if current_mode.browser_enabled() {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Min(10),
                        Constraint::Length(3),
                    ])
                    .split(f.area())[1]
            } else {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Length(16),
                        Constraint::Min(0),
                    ])
                    .split(f.area())[1]
            };
            ngrok_setup_copy_area = ngrok_auth_setup_copy_area(anchor_area);
            if current_mode.browser_enabled() {
                draw_browser_select(
                    f,
                    &browsers,
                    &supported_indices,
                    selected_supported_idx,
                    current_theme,
                );
            } else {
                draw_mode_select(f, current_theme, current_tool_mode);
            }
            draw_ngrok_auth_setup(
                f,
                current_theme,
                anchor_area,
                &config_path_text,
                &masked_secret_preview(&input),
                error_message.as_deref(),
            );
            if let Some((message, pos)) = toast_ref {
                render_toast(f, current_theme.palette, message, pos);
            }
        })?;

        if !event::poll(UI_POLL_INTERVAL)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => return Ok(false),
                    KeyCode::Enter => {
                        let token = normalize_ngrok_authtoken_input(&input);
                        if token.is_empty() {
                            error_message = Some("NGROK_AUTHTOKEN cannot be empty".into());
                            continue;
                        }
                        match save_ngrok_authtoken(&token) {
                            Ok(saved_path) => {
                                let mut app = state.lock().await;
                                app.log(
                                    "INFO",
                                    format!(
                                        "Saved ngrok authtoken to {}",
                                        saved_path.to_string_lossy()
                                    ),
                                );
                                return Ok(true);
                            }
                            Err(e) => {
                                error_message =
                                    Some(format!("Failed to save ~/.catdesk/config.toml: {e}"));
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        input.pop();
                        error_message = None;
                    }
                    KeyCode::Char(c) => {
                        if key_is_clipboard_paste(&key) {
                            if let Some(text) = clipboard_paste() {
                                input.push_str(&normalize_ngrok_authtoken_input(&text));
                                error_message = None;
                            }
                        } else {
                            input.push(c);
                            error_message = None;
                        }
                    }
                    KeyCode::Insert if key_is_clipboard_paste(&key) => {
                        if let Some(text) = clipboard_paste() {
                            input.push_str(&normalize_ngrok_authtoken_input(&text));
                            error_message = None;
                        }
                    }
                    _ => {}
                }
            }
            Event::Paste(text) => {
                input.push_str(&normalize_ngrok_authtoken_input(&text));
                error_message = None;
            }
            Event::Mouse(mouse) => {
                if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left))
                    && rect_contains(ngrok_setup_copy_area, mouse.column, mouse.row)
                {
                    let message = if clipboard_copy(NGROK_SETUP_URL) {
                        "Copied!"
                    } else {
                        "Copy failed"
                    };
                    toast = Some((message, (mouse.column, mouse.row), Instant::now()));
                }
            }
            _ => {}
        }
    }
}

fn masked_secret_preview(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = value.chars().collect();
    let visible = chars.len().min(4);
    let masked_len = chars.len().saturating_sub(visible);
    let mut preview = "*".repeat(masked_len);
    preview.extend(chars[chars.len() - visible..].iter());
    preview
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

fn ngrok_auth_setup_modal_area(anchor_area: Rect) -> Rect {
    centered_rect(90, 15, anchor_area)
}

fn ngrok_auth_setup_content_area(anchor_area: Rect) -> Rect {
    let modal_area = ngrok_auth_setup_modal_area(anchor_area);
    let inner = Rect::new(
        modal_area.x.saturating_add(1),
        modal_area.y.saturating_add(1),
        modal_area.width.saturating_sub(2),
        modal_area.height.saturating_sub(2),
    );
    inner.inner(Margin {
        horizontal: 2,
        vertical: 1,
    })
}

fn ngrok_auth_setup_copy_area(anchor_area: Rect) -> Rect {
    let content_area = ngrok_auth_setup_content_area(anchor_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(content_area);
    let body = chunks[0];
    if body.height <= 2 {
        return Rect::new(body.x, body.y, 0, 0);
    }
    Rect::new(body.x, body.y.saturating_add(2), body.width, 2)
}

fn draw_ngrok_auth_setup(
    f: &mut Frame,
    theme: &theme::ThemeDef,
    anchor_area: Rect,
    _config_path: &str,
    masked_value: &str,
    error_message: Option<&str>,
) {
    let palette = theme.palette;
    let modal_bg = Color::Rgb(34, 38, 47);
    let modal_fg = Color::Rgb(232, 236, 242);

    let modal_area = ngrok_auth_setup_modal_area(anchor_area);
    f.render_widget(Clear, modal_area);
    let modal_block = Block::default()
        .title(" ngrok auth ")
        .borders(Borders::ALL)
        .border_type(palette.border_type)
        .border_style(Style::default().fg(palette.border_fg))
        .style(Style::default().bg(modal_bg));
    f.render_widget(modal_block, modal_area);
    let content_area = ngrok_auth_setup_content_area(anchor_area);

    let modal_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(content_area);

    let link_style = Style::default()
        .fg(palette.primary_fg)
        .bg(modal_bg)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let step_style = Style::default()
        .fg(palette.title_fg)
        .bg(modal_bg)
        .add_modifier(Modifier::BOLD);
    let body_lines = vec![
        Line::from(Span::styled("ngrok setup required", step_style)),
        Line::from(""),
        Line::from(vec![
            Span::styled("1. Open in browser and get your authtoken", step_style),
            Span::raw(" "),
            Span::styled(
                "(click to copy)",
                Style::default().fg(palette.secondary_fg).bg(modal_bg),
            ),
        ]),
        Line::from(vec![
            Span::raw("   "),
            Span::styled(NGROK_SETUP_URL, link_style),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "2. Paste the token or ngrok config command below",
            step_style,
        )),
    ];
    let body = Paragraph::new(body_lines)
        .style(Style::default().fg(modal_fg).bg(modal_bg))
        .wrap(Wrap { trim: false });
    f.render_widget(body, modal_chunks[0]);

    let input_line = if masked_value.is_empty() {
        "_".to_string()
    } else {
        masked_value.to_string()
    };
    let input = Paragraph::new(format!("  {input_line}"))
        .style(Style::default().fg(palette.title_fg).bg(modal_bg))
        .block(
            Block::default()
                .title(" NGROK_AUTHTOKEN ")
                .borders(Borders::ALL)
                .border_type(palette.border_type)
                .border_style(Style::default().fg(palette.border_fg))
                .style(Style::default().bg(modal_bg)),
        );
    f.render_widget(input, modal_chunks[1]);

    let footer = if let Some(message) = error_message {
        Paragraph::new(Line::from(Span::styled(
            message.to_string(),
            Style::default().fg(palette.danger_fg).bg(modal_bg),
        )))
    } else {
        Paragraph::new(Line::from(Span::styled(
            "[Enter] Save  [q/Esc] Quit  [Paste/Ctrl+V] Insert token",
            Style::default().fg(palette.muted_fg).bg(modal_bg),
        )))
    };
    f.render_widget(footer, modal_chunks[2]);
}

fn render_toast(f: &mut Frame, palette: theme::Palette, msg: &str, pos: (u16, u16)) {
    let area = f.area();
    let (col, row) = pos;
    let label = format!(" {msg} ");
    let w = label.len() as u16;
    let x = col.saturating_add(1).min(area.width.saturating_sub(w));
    let y = if row > 0 { row - 1 } else { row + 1 }.min(area.height.saturating_sub(1));
    let toast_area = Rect::new(x, y, w, 1);
    let toast_widget = Paragraph::new(label).style(
        Style::default()
            .bg(palette.toast_bg)
            .fg(palette.toast_fg)
            .add_modifier(Modifier::BOLD),
    );
    f.render_widget(toast_widget, toast_area);
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::fail_owned_candidate_after_health_check;
    #[cfg(windows)]
    use super::format_candidate_owner_diagnostic_report;
    #[cfg(windows)]
    use super::verify_reviewed_candidate_listener_owner;
    #[cfg(windows)]
    use super::verify_reviewed_candidate_service_health;
    use super::{
        ReviewedCandidateParentControl, ToolMode, acquire_reviewed_candidate_process_lock,
        is_canonical_production_worker_listener, key_is_clipboard_paste, loopback_http_origin,
        normalize_ngrok_authtoken_input, operator_web_error_code, parse_auto_start_computer,
        parse_headless_mcp_options, parse_native_daemon_mode, parse_owned_candidate_ready,
        parse_reviewed_candidate_mcp_options, parse_reviewed_candidate_owner_diagnose_options,
        parse_reviewed_candidate_owner_options, parse_reviewed_candidate_parent_control,
        probe_reviewed_candidate_ready, resolve_headless_workspace, safe_bind_identity,
        validate_headless_mcp_path, validate_isolated_fixture_scope,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
    use std::time::Duration;

    #[test]
    fn operator_web_attention_exposes_only_bounded_stable_codes() {
        assert_eq!(
            operator_web_error_code("BROWSER_SESSION_BUSY"),
            "BROWSER_SESSION_BUSY"
        );
        assert_eq!(
            operator_web_error_code("BROWSER_SESSION_BUSY: C:\\private\\profile"),
            "BROWSER_SESSION_BUSY"
        );
        assert_eq!(
            operator_web_error_code("DEV_LOGIN_CLEANUP_UNVERIFIED"),
            "DEV_LOGIN_CLEANUP_UNVERIFIED"
        );
        assert_eq!(
            operator_web_error_code("WEB_USAGE: invalid project ID"),
            "WEB_USAGE"
        );
        assert_eq!(
            operator_web_error_code("WEB_TARGET: C:\\private\\target.json"),
            "WEB_TARGET"
        );
        for code in [
            "RESPONSE_EXACT_USER_MISSING_OR_AMBIGUOUS",
            "LOGIN_OR_PROFILE_REQUIRED",
            "COOPERATIVE_HOST_PROTOCOL_REQUIRED",
            "HOST_EXECUTABLE_HASH_MISMATCH",
        ] {
            assert_eq!(operator_web_error_code(code), code);
            assert_eq!(
                operator_web_error_code(&format!("{code}: C:\\private\\profile")),
                code
            );
        }
        for untrusted in [
            "C:\\private\\token.txt",
            "raw task text with a secret",
            "SECRET_VALUE: private token",
            "WEB_USAGE raw task text",
            "web_scope: private path",
            "WEB_SCOPE\nprivate path",
            "RESPONSE_PRIVATE\nsecret",
            "LOGIN_OR_PROFILE_REQUIRED private profile text",
        ] {
            assert_eq!(operator_web_error_code(untrusted), "WEB_OPERATION_REFUSED");
        }
        let oversized = format!("WEB_{}", "A".repeat(61));
        assert_eq!(operator_web_error_code(&oversized), "WEB_OPERATION_REFUSED");
    }

    #[test]
    fn reviewed_candidate_parent_control_is_exact() {
        assert_eq!(
            parse_reviewed_candidate_parent_control(b"stop\n"),
            Ok(ReviewedCandidateParentControl::Stop)
        );
        assert_eq!(
            parse_reviewed_candidate_parent_control(b"fail\n"),
            Ok(ReviewedCandidateParentControl::HealthFailure)
        );
        for command in [b"quit\n", b"stop!", b"FAIL\n", b"fail "] {
            assert!(parse_reviewed_candidate_parent_control(command).is_err());
        }
    }

    #[cfg(windows)]
    #[test]
    fn reviewed_candidate_failure_control_child() {
        use std::io::Read;
        let Ok(mode) = std::env::var("CATDESK_REVIEWED_CANDIDATE_FAILURE_CHILD") else {
            return;
        };
        match mode.as_str() {
            "respond" => {
                let mut command = [0u8; 5];
                std::io::stdin()
                    .read_exact(&mut command)
                    .expect("receive private failure command");
                std::process::exit(if &command == b"fail\n" { 17 } else { 18 });
            }
            "stall" => std::thread::sleep(Duration::from_secs(60)),
            _ => panic!("unexpected failure control test mode"),
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn reviewed_candidate_health_failure_bounds_responsive_and_stalled_children() {
        use super::reviewed_candidate_owner::OwnedCandidateChild;
        use std::ffi::c_void;
        use std::process::{Command, Stdio};

        const SYNCHRONIZE: u32 = 0x0010_0000;
        const WAIT_OBJECT_0: u32 = 0;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
            fn WaitForSingleObject(handle: *mut c_void, timeout_ms: u32) -> u32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }

        for (mode, expected, wait_limit) in [
            (
                "respond",
                "child exited after failure command (exit code: 17)",
                Duration::from_secs(3),
            ),
            (
                "stall",
                "child did not exit after failure command",
                Duration::from_millis(200),
            ),
        ] {
            let mut command = Command::new(std::env::current_exe().expect("test executable"));
            command
                .args([
                    "--exact",
                    "tests::reviewed_candidate_failure_control_child",
                    "--nocapture",
                ])
                .env("CATDESK_REVIEWED_CANDIDATE_FAILURE_CHILD", mode)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let owned = OwnedCandidateChild::spawn(&mut command).expect("own test child");
            let process = unsafe { OpenProcess(SYNCHRONIZE, 0, owned.pid()) };
            assert!(
                !process.is_null(),
                "retain exact child process for read-only check"
            );
            let started = std::time::Instant::now();
            let error = fail_owned_candidate_after_health_check(
                owned,
                "fixture service probe failed",
                wait_limit,
            )
            .await;
            assert!(error.to_string().contains(expected), "{error}");
            assert!(
                error
                    .to_string()
                    .contains("journal requires read-only diagnosis")
            );
            assert!(started.elapsed() < Duration::from_secs(8));
            assert_eq!(unsafe { WaitForSingleObject(process, 0) }, WAIT_OBJECT_0);
            assert_ne!(unsafe { CloseHandle(process) }, 0);
        }
    }

    #[cfg(windows)]
    #[test]
    fn reviewed_candidate_process_lock_child() {
        if std::env::var_os("CATDESK_REVIEWED_CANDIDATE_LOCK_TEST_CHILD").is_none() {
            return;
        }
        let _lock = acquire_reviewed_candidate_process_lock().expect("own candidate lock");
        println!("LOCK_READY");
        std::io::Write::flush(&mut std::io::stdout()).expect("flush ready");
        std::thread::sleep(Duration::from_secs(60));
    }

    #[cfg(windows)]
    #[test]
    fn reviewed_candidate_process_lock_excludes_second_process_and_releases_on_crash() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "tests::reviewed_candidate_process_lock_child",
                "--nocapture",
            ])
            .env("CATDESK_REVIEWED_CANDIDATE_LOCK_TEST_CHILD", "1")
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn candidate lock owner");
        let mut reader = BufReader::new(child.stdout.take().expect("child stdout"));
        let mut line = String::new();
        let mut ready = false;
        while reader.read_line(&mut line).expect("child output") != 0 {
            if line.contains("LOCK_READY") {
                ready = true;
                break;
            }
            line.clear();
        }
        if !ready {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child never acquired the candidate lock");
        }
        let contender_rejected = acquire_reviewed_candidate_process_lock().is_err();
        child.kill().expect("hard kill lock owner");
        child.wait().expect("reap owner");
        assert!(
            contender_rejected,
            "second process must fail before staging"
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(lock) = acquire_reviewed_candidate_process_lock() {
                drop(lock);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "crashed owner did not release exact kernel lock"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    #[test]
    fn reviewed_candidate_parent_accepts_only_exact_child_scope() {
        let args = [
            "--reviewed-candidate-owner",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--mcp-path",
            "/dev/mcp",
            "--workspace",
            "C:\\source",
            "--expected-commit",
            "a123456789012345678901234567890123456789",
            "--auth-token",
            "0123456789abcdefghijklmnop",
        ];
        let args = args.map(str::to_owned).to_vec();
        assert!(
            parse_reviewed_candidate_owner_options(&args)
                .expect("exact parent scope")
                .is_some()
        );
        for (old, new) in [
            ("127.0.0.1", "0.0.0.0"),
            ("/dev/mcp", "/mcp"),
            ("a123456789012345678901234567890123456789", "bad"),
        ] {
            let changed = args
                .iter()
                .map(|arg| {
                    if arg == old {
                        new.to_owned()
                    } else {
                        arg.clone()
                    }
                })
                .collect::<Vec<_>>();
            assert!(parse_reviewed_candidate_owner_options(&changed).is_err());
        }
        let mut duplicate = args;
        duplicate.push("--reviewed-candidate-mcp".into());
        assert!(parse_reviewed_candidate_owner_options(&duplicate).is_err());
    }

    #[test]
    fn reviewed_candidate_parent_ready_is_bound_to_exact_pid_commit_and_loopback() {
        let commit = "a123456789012345678901234567890123456789";
        let ready = serde_json::json!({
            "status":"ready",
            "kind":"reviewed-candidate",
            "snapshotId":"snapshot-1",
            "sourceCommit":commit,
            "runtimeGeneration":"generation-1",
            "ownerPid":1234,
            "mcpUrl":"http://127.0.0.1:41777/dev/mcp",
            "auth":{"scheme":"bearer","tokenRedacted":true}
        });
        assert_eq!(
            parse_owned_candidate_ready(&ready.to_string(), commit, 1234)
                .expect("exact child record")
                .port(),
            41777
        );
        assert!(parse_owned_candidate_ready(&ready.to_string(), commit, 1235).is_err());
        let mut changed = ready.clone();
        changed["sourceCommit"] = serde_json::json!("b".repeat(40));
        assert!(parse_owned_candidate_ready(&changed.to_string(), commit, 1234).is_err());
        for url in [
            "http://127.0.0.2:41777/dev/mcp",
            "http://127.0.0.1:41777/dev/mcp?x=1",
            "http://127.0.0.1:41777/other",
            "https://127.0.0.1:41777/dev/mcp",
            "http://user@127.0.0.1:41777/dev/mcp",
        ] {
            changed = ready.clone();
            changed["mcpUrl"] = serde_json::json!(url);
            assert!(parse_owned_candidate_ready(&changed.to_string(), commit, 1234).is_err());
        }
    }

    #[tokio::test]
    async fn reviewed_candidate_ready_probe_requires_authenticated_isolated_mcp() {
        use super::{AppState, SharedState, server};
        use std::sync::Arc;
        use tokio::sync::{Mutex, mpsc::unbounded_channel};

        let root = std::env::temp_dir().join(format!(
            "catdesk-ready-probe-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).expect("test root");
        let app = AppState::new_isolated_fixture(
            0,
            root.to_string_lossy().into_owned(),
            root.join("config.toml"),
        )
        .expect("isolated app");
        let state: SharedState = Arc::new(Mutex::new(app));
        let (tx, _rx) = unbounded_channel();
        let router = server::isolated_fixture_router(
            state,
            "/dev/mcp".into(),
            tx,
            "fixture-private-token".into(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener");
        let addr = listener.local_addr().expect("bound address");
        let task = tokio::spawn(async move { axum::serve(listener, router).await });
        probe_reviewed_candidate_ready(addr, "fixture-private-token")
            .await
            .expect("authenticated isolated read/search catalog");
        #[cfg(windows)]
        verify_reviewed_candidate_service_health(addr, "fixture-private-token", std::process::id())
            .await
            .expect("healthy route remains owned by this process");
        assert!(
            probe_reviewed_candidate_ready(addr, "wrong-token")
                .await
                .is_err(),
            "wrong bearer token cannot prove readiness"
        );
        task.abort();
        let _ = task.await;
        std::fs::remove_dir_all(root).expect("remove exact test root");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn reviewed_candidate_service_watch_rejects_wedged_listener_with_live_pid() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener");
        let address = listener.local_addr().expect("bound address");
        verify_reviewed_candidate_listener_owner(address, std::process::id())
            .expect("socket still belongs to live test process");
        let stalled = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept health probe");
            tokio::time::sleep(Duration::from_secs(10)).await;
        });
        let started = std::time::Instant::now();
        let outcome = tokio::time::timeout(
            Duration::from_secs(8),
            verify_reviewed_candidate_service_health(
                address,
                "fixture-private-token",
                std::process::id(),
            ),
        )
        .await
        .expect("owner health watch has an outer deadline");
        assert!(
            outcome.is_err(),
            "a bound but wedged listener is not healthy"
        );
        assert!(started.elapsed() < Duration::from_secs(8));
        stalled.abort();
        let _ = stalled.await;
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn reviewed_candidate_listener_owner_must_match_live_socket_pid() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind candidate loopback listener");
        let address = listener.local_addr().expect("candidate address");
        verify_reviewed_candidate_listener_owner(address, std::process::id())
            .expect("Windows reports this process as the listener owner");
        assert!(
            verify_reviewed_candidate_listener_owner(address, u32::MAX).is_err(),
            "foreign process ID must not pass owner verification"
        );
        assert!(
            verify_reviewed_candidate_listener_owner(
                SocketAddr::from(([0, 0, 0, 0], address.port())),
                std::process::id(),
            )
            .is_err(),
            "non-loopback address must not pass owner verification"
        );
        drop(listener);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if verify_reviewed_candidate_listener_owner(address, std::process::id()).is_err() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "closed listener remained assigned to this PID"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    #[test]
    fn auto_start_computer_is_explicit_and_singleton() {
        assert_eq!(
            parse_auto_start_computer(&["--auto-start-computer".to_string()]),
            Ok(true)
        );
        assert_eq!(parse_auto_start_computer(&[]), Ok(false));
        assert!(
            parse_auto_start_computer(&[
                "--auto-start-computer".to_string(),
                "--auto-start-computer".to_string(),
            ])
            .is_err()
        );
    }

    #[test]
    fn native_daemon_mode_is_explicit_bounded_and_terminal_free() {
        assert_eq!(
            parse_native_daemon_mode(&[crate::daemon_reload::DAEMON_MODE_FLAG.to_string()]),
            Ok(true)
        );
        assert_eq!(parse_native_daemon_mode(&[]), Ok(false));
        assert!(
            parse_native_daemon_mode(&[
                crate::daemon_reload::DAEMON_MODE_FLAG.to_string(),
                "--auto-start-computer".to_string(),
            ])
            .is_err()
        );
    }

    #[test]
    fn fixed_supervisor_registration_is_limited_to_the_canonical_worker_listener() {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};

        assert!(is_canonical_production_worker_listener(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            crate::control_plane_supervisor::FIXED_LOCAL_BACKEND_PORT,
        )));
        assert!(!is_canonical_production_worker_listener(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            3202,
        )));
        assert!(!is_canonical_production_worker_listener(&SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            crate::control_plane_supervisor::FIXED_LOCAL_BACKEND_PORT,
        )));
        let source = include_str!("main.rs");
        assert!(source.contains("register_fixed_ready_worker_listener("));
        assert!(source.contains("verified_current_reviewed_main_image_digest"));
        assert!(source.contains("is_canonical_production_worker_listener(&local_addr)"));
        assert!(
            !source.contains(&["register_fixed_ready_worker_listener", "(&TcpListener"].concat())
        );
    }

    #[test]
    fn production_argument_routing_references_the_native_gui_mode() {
        let source = include_str!("main.rs");
        assert!(source.contains("mod windows_gui;"));
        assert!(source.contains("windows_gui::parse_binagotchy_gui_mode(&args)"));
        assert!(source.contains("windows_gui::run_gui(&workspace)"));
    }

    #[test]
    fn windows_bare_launch_is_presentation_only_before_legacy_app_state() {
        let source = include_str!("main.rs");
        let bare_launch = source
            .find("    if args.is_empty() {")
            .expect("Windows bare-launch routing");
        let legacy_state = bare_launch
            + source[bare_launch..]
                .find("let state: SharedState = Arc::new(Mutex::new(AppState::new(port, workspace_root)?));")
                .expect("legacy TUI AppState construction after bare-launch routing");
        let route = &source[bare_launch..legacy_state];
        assert!(route.contains("binagotchy_cli::launch_associated"));
        assert!(route.contains("return Ok(())"));
    }
    use std::time::SystemTime;

    #[test]
    fn normalizes_plain_ngrok_token() {
        assert_eq!(
            normalize_ngrok_authtoken_input("  test-token-123  "),
            "test-token-123"
        );
    }

    #[test]
    fn extracts_token_from_ngrok_command() {
        assert_eq!(
            normalize_ngrok_authtoken_input("ngrok config add-authtoken test-token-123"),
            "test-token-123"
        );
    }

    #[test]
    fn detects_ctrl_v_as_clipboard_paste() {
        assert!(key_is_clipboard_paste(&KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::CONTROL
        )));
    }

    #[test]
    fn detects_shift_insert_as_clipboard_paste() {
        assert!(key_is_clipboard_paste(&KeyEvent::new(
            KeyCode::Insert,
            KeyModifiers::SHIFT
        )));
    }

    #[test]
    fn reviewed_candidate_mode_requires_exact_local_identity() {
        let args = [
            "--reviewed-candidate-mcp",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--mcp-path",
            "/dev/mcp",
            "--workspace",
            "C:\\reviewed-source",
            "--expected-commit",
            "abcdef0123456789abcdef0123456789abcdef01",
            "--auth-token",
            "catdesk-reviewed-token-1234567890",
        ]
        .map(str::to_owned);
        let parsed = parse_reviewed_candidate_mcp_options(args.clone())
            .expect("parse fixed reviewed mode")
            .expect("reviewed mode selected");
        assert_eq!(
            parsed.expected_commit,
            "abcdef0123456789abcdef0123456789abcdef01"
        );
        for extra in [
            vec!["--host", "0.0.0.0"],
            vec!["--port", "3200"],
            vec!["--mcp-path", "/prod/mcp"],
            vec!["--headless-mcp"],
            vec!["--config-path", "C:\\production\\config.toml"],
        ] {
            let mut candidate = args.to_vec();
            candidate.extend(extra.into_iter().map(str::to_owned));
            assert!(parse_reviewed_candidate_mcp_options(candidate).is_err());
        }
        let mut bad_commit = args.to_vec();
        let position = bad_commit
            .iter()
            .position(|arg| arg == "--expected-commit")
            .unwrap();
        bad_commit[position + 1] = "ABCDEF0123456789ABCDEF0123456789ABCDEF01".into();
        assert!(parse_reviewed_candidate_mcp_options(bad_commit).is_err());
        let mut missing_token = args.to_vec();
        let position = missing_token
            .iter()
            .position(|arg| arg == "--auth-token")
            .unwrap();
        missing_token.truncate(position);
        assert!(parse_reviewed_candidate_mcp_options(missing_token).is_err());
    }

    #[test]
    fn candidate_owner_diagnosis_accepts_one_exact_source_and_snapshot_scope() {
        let args = [
            "--reviewed-candidate-owner-diagnose",
            "--source-workspace",
            "C:\\reviewed-source",
            "--expected-commit",
            "abcdef0123456789abcdef0123456789abcdef01",
            "--snapshot-id",
            "8e1ae4ac-3d5f-46b3-9d5f-1e16bbf4f3e5",
        ]
        .map(str::to_owned);
        let parsed = parse_reviewed_candidate_owner_diagnose_options(&args)
            .expect("strict diagnostic options")
            .expect("diagnostic selected");
        assert_eq!(
            parsed.source_workspace,
            std::path::PathBuf::from("C:\\reviewed-source")
        );
        assert_eq!(parsed.snapshot_id, "8e1ae4ac-3d5f-46b3-9d5f-1e16bbf4f3e5");
        assert!(
            parse_reviewed_candidate_owner_diagnose_options(&[])
                .unwrap()
                .is_none()
        );

        for extra in [
            vec!["--reviewed-candidate-owner-diagnose"],
            vec!["--snapshot-id", "8e1ae4ac-3d5f-46b3-9d5f-1e16bbf4f3e5"],
            vec!["--reviewed-candidate-owner"],
            vec!["--reviewed-candidate-mcp"],
            vec!["--headless-mcp"],
            vec!["--journal", "C:\\other"],
            vec!["--cleanup"],
        ] {
            let mut changed = args.to_vec();
            changed.extend(extra.into_iter().map(str::to_owned));
            assert!(parse_reviewed_candidate_owner_diagnose_options(&changed).is_err());
        }
        for (option, invalid) in [
            ("--source-workspace", ""),
            ("--source-workspace", "--reviewed-candidate-owner-diagnose"),
            ("--expected-commit", "HEAD"),
            ("--expected-commit", "--snapshot-id"),
            (
                "--expected-commit",
                "ABCDEF0123456789ABCDEF0123456789ABCDEF01",
            ),
            ("--snapshot-id", "8E1AE4AC-3D5F-46B3-9D5F-1E16BBF4F3E5"),
            ("--snapshot-id", "../catdesk-dev-fixture-other"),
            ("--snapshot-id", "--cleanup"),
        ] {
            let mut changed = args.to_vec();
            let position = changed.iter().position(|arg| arg == option).unwrap();
            changed[position + 1] = invalid.into();
            assert!(parse_reviewed_candidate_owner_diagnose_options(&changed).is_err());
        }
        for required in ["--source-workspace", "--expected-commit", "--snapshot-id"] {
            let mut changed = args.to_vec();
            let position = changed.iter().position(|arg| arg == required).unwrap();
            changed.drain(position..=position + 1);
            assert!(parse_reviewed_candidate_owner_diagnose_options(&changed).is_err());
        }
        let swallowed_marker = [
            "--source-workspace",
            "--reviewed-candidate-owner-diagnose",
            "--expected-commit",
            "abcdef0123456789abcdef0123456789abcdef01",
            "--snapshot-id",
            "8e1ae4ac-3d5f-46b3-9d5f-1e16bbf4f3e5",
        ]
        .map(str::to_owned);
        assert!(parse_reviewed_candidate_owner_diagnose_options(&swallowed_marker).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn candidate_owner_no_journal_report_names_observed_parent_and_witness_time_scope() {
        use crate::isolated_fixture_snapshot::{
            CandidateOwnerRecoveryClass, CandidateOwnerRecoveryReport,
        };
        let report = CandidateOwnerRecoveryReport {
            snapshot_id: "8e1ae4ac-3d5f-46b3-9d5f-1e16bbf4f3e5".into(),
            class: CandidateOwnerRecoveryClass::NoJournal,
            detail: None,
        };
        let output = format_candidate_owner_diagnostic_report(
            report,
            "abcdef0123456789abcdef0123456789abcdef01",
            42,
            1,
            "C:\\Users\\operator\\AppData\\Local\\Temp",
        )
        .expect("format diagnostic scope");
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["classification"], "NO_JOURNAL");
        assert_eq!(value["witnessScope"], "HISTORICAL_COMMIT_PUSH_CI");
        assert_eq!(value["witnessRunAttempt"], 1);
        assert_eq!(
            value["scope"],
            "EXACT_SNAPSHOT_IN_DIAGNOSTIC_PROCESS_TEMP_PARENT"
        );
        assert_eq!(
            value["tempParent"],
            "C:\\Users\\operator\\AppData\\Local\\Temp"
        );
        assert_eq!(value["authority"], "DIAGNOSTIC_ONLY");
    }

    #[test]
    fn isolated_dev_fixture_mode_requires_strict_headless_identity() {
        let args = [
            "--headless-mcp",
            "--isolated-dev-fixture",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--mcp-path",
            "/dev/mcp",
            "--workspace",
            "C:\\Temp\\catdesk-dev-fixture-123",
            "--config-path",
            "C:\\Temp\\catdesk-dev-fixture-123\\config.toml",
            "--tool-mode",
            "read-only",
            "--auth-token",
            "catdesk-fixture-token-1234567890",
        ]
        .map(str::to_owned);
        let parsed = parse_headless_mcp_options(args.clone())
            .expect("parse fixture")
            .expect("fixture selected");
        assert!(parsed.isolated_dev_fixture);
        assert_eq!(parsed.port, 0);
        for extra in [
            vec!["--tool-mode", "multi-tools"],
            vec!["--port", "3200"],
            vec!["--mcp-path", "/prod/mcp"],
            vec!["--mode", "browser"],
        ] {
            let mut candidate = args.to_vec();
            candidate.extend(extra.into_iter().map(str::to_owned));
            assert!(parse_headless_mcp_options(candidate).is_err());
        }
        assert!(
            parse_headless_mcp_options([
                "--headless-mcp".into(),
                "--isolated-dev-fixture".into(),
                "--auth-token".into(),
                "catdesk-fixture-token-1234567890".into(),
                "--config-path".into(),
                "config.toml".into(),
            ])
            .is_err()
        );
    }

    #[test]
    fn isolated_fixture_config_is_memory_only_even_with_existing_file() {
        let root = std::env::temp_dir().join(format!(
            "catdesk-dev-fixture-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).expect("create disposable fixture");
        let config = root.join("config.toml");
        let workspace = root.to_str().expect("fixture path");
        let mut state =
            crate::state::AppState::new_isolated_fixture(0, workspace.to_owned(), config.clone())
                .expect("in-memory fixture state");
        assert!(!config.exists(), "default fixture must not create a config");
        assert!(
            state.persist_state().is_err(),
            "fixture persistence must fail closed"
        );
        state.persist_state_with_log();
        assert!(
            !config.exists(),
            "logging persistence refusal must not write"
        );

        let marker = "not valid TOML [";
        std::fs::write(&config, marker).expect("create untrusted fixture config");
        let mut ignored =
            crate::state::AppState::new_isolated_fixture(0, workspace.to_owned(), config.clone())
                .expect("fixture ignores pathname-based config");
        assert!(ignored.persist_state().is_err());
        ignored.persist_state_with_log();
        assert_eq!(std::fs::read_to_string(&config).unwrap(), marker);
        std::fs::remove_dir_all(root).expect("cleanup own fixture");
    }

    #[test]
    fn isolated_dev_fixture_rejects_production_and_outside_config() {
        let fixture = std::env::temp_dir().join(format!(
            "catdesk-dev-fixture-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&fixture).expect("create fixture");
        let root = std::fs::canonicalize(&fixture).expect("canonical fixture");
        let path = root.to_str().expect("utf8 fixture");
        assert!(validate_isolated_fixture_scope(path, &root.join("config.toml")).is_ok());
        #[cfg(windows)]
        {
            // A second name for external configuration must not be accepted.
            let external = std::env::temp_dir().join(format!(
                "catdesk-external-config-{}.toml",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::write(&external, "external=true").expect("external fixture config");
            let linked_config = root.join("config.toml");
            std::fs::hard_link(&external, &linked_config).expect("fixture config hardlink");
            assert!(validate_isolated_fixture_scope(path, &linked_config).is_err());
            std::fs::remove_file(&linked_config).expect("remove fixture config hardlink");
            std::fs::remove_file(&external).expect("remove external fixture config");
        }
        assert!(validate_isolated_fixture_scope(path, &root.join("other.toml")).is_err());
        assert!(
            validate_isolated_fixture_scope(path, &std::env::temp_dir().join("config.toml"))
                .is_err()
        );
        let invalid = std::env::temp_dir().join(format!(
            "not-catdesk-fixture-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&invalid).expect("invalid folder");
        let canonical = std::fs::canonicalize(&invalid).expect("canonical invalid");
        assert!(
            validate_isolated_fixture_scope(
                canonical.to_str().expect("utf8"),
                &canonical.join("config.toml")
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(fixture);
        let _ = std::fs::remove_dir_all(invalid);
    }

    #[test]
    fn parses_headless_mcp_options_with_disposable_config() {
        let options = parse_headless_mcp_options([
            "--headless-mcp".to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
            "--port".to_string(),
            "33200".to_string(),
            "--workspace".to_string(),
            "C:\\Temp\\catdesk-disposable".to_string(),
            "--mcp-path".to_string(),
            "/t0012/mcp".to_string(),
            "--mode".to_string(),
            "computer".to_string(),
            "--tool-mode".to_string(),
            "read-only".to_string(),
            "--config-path".to_string(),
            ".tmp\\catdesk-headless\\config.toml".to_string(),
            "--auth-token".to_string(),
            "catdesk-test-token-1234567890".to_string(),
            "--no-ngrok".to_string(),
            "--max-runtime-seconds".to_string(),
            "300".to_string(),
            "--web-auto-project".to_string(),
            "catdesk".to_string(),
            "--web-auto-project".to_string(),
            "wake2".to_string(),
        ])
        .expect("parse headless options")
        .expect("headless options present");

        assert_eq!(options.host, "127.0.0.1");
        assert_eq!(options.port, 33200);
        assert_eq!(options.mcp_path.as_deref(), Some("/t0012/mcp"));
        assert!(matches!(options.tool_mode, ToolMode::ReadOnly));
        assert_eq!(options.auth_token, "catdesk-test-token-1234567890");
        assert_eq!(options.maximum_runtime_seconds, Some(300));
        assert_eq!(
            options.web_auto_projects.into_iter().collect::<Vec<_>>(),
            ["catdesk", "wake2"]
        );
    }

    #[test]
    fn headless_budget_and_project_scope_refuse_invalid_or_duplicate_options() {
        for tail in [
            vec!["--max-runtime-seconds", "0"],
            vec!["--max-runtime-seconds", "901"],
            vec!["--max-runtime-seconds", "-1"],
            vec!["--max-runtime-seconds", "1", "--max-runtime-seconds", "2"],
            vec!["--web-auto-project", "../other"],
            vec!["--web-auto-project", "alpha", "--web-auto-project", "alpha"],
        ] {
            let mut args = vec![
                "--headless-mcp",
                "--config-path",
                "unused.toml",
                "--auth-token",
                "catdesk-test-token-1234567890",
            ];
            args.extend(tail);
            assert!(parse_headless_mcp_options(args.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    fn omitted_headless_tool_mode_defaults_to_read_only() {
        let options = parse_headless_mcp_options([
            "--headless-mcp".to_string(),
            "--config-path".to_string(),
            ".tmp\\catdesk-headless\\config.toml".to_string(),
            "--auth-token".to_string(),
            "catdesk-test-token-1234567890".to_string(),
        ])
        .expect("parse headless options")
        .expect("headless options present");

        assert!(matches!(options.tool_mode, ToolMode::ReadOnly));
        assert_eq!(options.maximum_runtime_seconds, None);
        assert!(options.web_auto_projects.is_empty());
    }

    #[test]
    fn explicit_headless_read_only_succeeds() {
        let options = parse_headless_mcp_options([
            "--headless-mcp".to_string(),
            "--tool-mode".to_string(),
            "read-only".to_string(),
            "--config-path".to_string(),
            ".tmp\\catdesk-headless\\config.toml".to_string(),
            "--auth-token".to_string(),
            "catdesk-test-token-1234567890".to_string(),
        ])
        .expect("parse read-only headless options")
        .expect("headless options present");

        assert!(matches!(options.tool_mode, ToolMode::ReadOnly));
    }

    #[test]
    fn valid_conservative_headless_mcp_paths_succeed() {
        for path in ["/t0012/mcp", "/ABC_123-def/mcp"] {
            assert_eq!(validate_headless_mcp_path(path).as_deref(), Ok(path));
        }
    }

    #[test]
    fn invalid_headless_mcp_paths_are_rejected() {
        for path in [
            "t0012/mcp",
            "/t0012",
            "/t0012/extra/mcp",
            "/../mcp",
            "/.%2e/mcp",
            "/hello world/mcp",
            "/hello.world/mcp",
            "/hello%20world/mcp",
            "/hello?x=1/mcp",
            "/hello#mcp/mcp",
            "/hello/mcp/",
        ] {
            assert!(
                validate_headless_mcp_path(path).is_err(),
                "expected rejection for {path}"
            );
        }
    }

    #[test]
    fn rejects_missing_headless_workspace() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let missing = std::env::temp_dir().join(format!("catdesk-missing-workspace-{unique}"));

        let error = resolve_headless_workspace(&missing.to_string_lossy())
            .expect_err("missing workspace should be rejected");

        assert!(error.to_string().contains("must exist"));
    }

    #[test]
    fn rejects_non_directory_headless_workspace() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let file_path = std::env::temp_dir().join(format!("catdesk-workspace-file-{unique}.txt"));
        std::fs::write(&file_path, "not a directory").expect("write temp file");

        let error = resolve_headless_workspace(&file_path.to_string_lossy())
            .expect_err("file workspace should be rejected");

        assert!(error.to_string().contains("must be a directory"));
        let _ = std::fs::remove_file(file_path);
    }

    #[test]
    fn rejects_headless_non_loopback_host() {
        let result = parse_headless_mcp_options([
            "--headless-mcp".to_string(),
            "--host".to_string(),
            "0.0.0.0".to_string(),
            "--config-path".to_string(),
            ".tmp\\catdesk-headless\\config.toml".to_string(),
            "--auth-token".to_string(),
            "catdesk-test-token-1234567890".to_string(),
        ]);
        let error = match result {
            Ok(_) => panic!("expected non-loopback host rejection"),
            Err(error) => error,
        };

        assert!(error.contains("loopback"));
    }

    #[test]
    fn rejects_headless_without_disposable_config_path() {
        let result = parse_headless_mcp_options(["--headless-mcp".to_string()]);
        let error = match result {
            Ok(_) => panic!("expected missing config path rejection"),
            Err(error) => error,
        };

        assert!(error.contains("--config-path"));
    }

    #[test]
    fn rejects_headless_without_auth_token() {
        let result = parse_headless_mcp_options([
            "--headless-mcp".to_string(),
            "--config-path".to_string(),
            ".tmp\\catdesk-headless\\config.toml".to_string(),
        ]);
        let error = match result {
            Ok(_) => panic!("expected missing auth token rejection"),
            Err(error) => error,
        };

        assert!(error.contains("--auth-token"));
    }

    #[test]
    fn rejects_headless_browser_mode() {
        let result = parse_headless_mcp_options([
            "--headless-mcp".to_string(),
            "--mode".to_string(),
            "both".to_string(),
            "--config-path".to_string(),
            ".tmp\\catdesk-headless\\config.toml".to_string(),
        ]);
        let error = match result {
            Ok(_) => panic!("expected browser mode rejection"),
            Err(error) => error,
        };

        assert!(error.contains("computer only"));
    }

    #[test]
    fn startup_scripts_use_environment_based_auth() {
        for script in [
            include_str!("../scripts/start-local-orchestrator.ps1"),
            include_str!("../scripts/start-dev-orchestrator.ps1"),
        ] {
            assert!(script.contains("CATDESK_MCP_AUTH_TOKEN"));
            assert!(script.contains("AuthToken is required"));
            assert!(script.contains("finally"));
            assert!(script.contains("Remove-Item Env:\\CATDESK_MCP_AUTH_TOKEN"));
            assert!(!script.contains("--auth-token"));
        }
    }

    #[test]
    fn default_runtime_bind_identity_is_loopback_and_port_exclusive() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
        let addr = listener.local_addr().expect("addr");
        assert!(addr.ip().is_loopback());
        assert_eq!(
            safe_bind_identity(&addr),
            format!("loopback:{}", addr.port())
        );
        assert!(TcpListener::bind(("127.0.0.1", addr.port())).is_err());
    }

    #[test]
    fn loopback_listener_is_not_reachable_on_lan_interface() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
        let port = listener.local_addr().expect("addr").port();
        let Some(lan_ip) = discover_best_effort_lan_ip() else {
            return;
        };
        let lan_addr = SocketAddr::new(lan_ip, port);
        assert!(
            TcpStream::connect_timeout(&lan_addr, Duration::from_millis(150)).is_err(),
            "loopback-only listener unexpectedly accepted LAN connection"
        );
    }

    #[test]
    fn loopback_http_origin_handles_ipv4_hostname_and_ipv6() {
        assert_eq!(
            loopback_http_origin("127.0.0.1", 3201),
            "http://127.0.0.1:3201"
        );
        assert_eq!(
            loopback_http_origin("localhost", 3201),
            "http://localhost:3201"
        );
        assert_eq!(loopback_http_origin("::1", 3201), "http://[::1]:3201");
    }

    fn discover_best_effort_lan_ip() -> Option<std::net::IpAddr> {
        let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("8.8.8.8:80").ok()?;
        let ip = socket.local_addr().ok()?.ip();
        (!ip.is_loopback()).then_some(ip)
    }
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let width = area
        .width
        .saturating_mul(percent_x)
        .saturating_div(100)
        .max(44);
    let width = width.min(area.width.saturating_sub(2).max(1));
    let popup_height = height.min(area.height.saturating_sub(2).max(1));
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(popup_height) / 2;
    Rect::new(x, y, width, popup_height)
}

async fn run_settings(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: SharedState,
) -> Result<(), Box<dyn std::error::Error>> {
    let themes = theme::all();
    let tool_modes = ToolMode::all();
    let mut confirm_reset_token_billing = false;
    let mut selected_row = {
        let app = state.lock().await;
        themes.iter().position(|t| t.id == app.theme).unwrap_or(0)
    };
    let total_rows = themes.len() + tool_modes.len() + 1;

    loop {
        let (current_theme, current_tool_mode, usage_totals, set_catdesk_as_co_author) = {
            let app = state.lock().await;
            (
                app.current_theme(),
                app.tool_mode,
                app.usage_totals.clone(),
                app.set_catdesk_as_co_author,
            )
        };
        terminal.draw(|f| {
            draw_settings(
                f,
                current_theme,
                current_tool_mode,
                set_catdesk_as_co_author,
                &usage_totals,
                selected_row,
                confirm_reset_token_billing,
            )
        })?;

        if event::poll(UI_POLL_INTERVAL)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Up => {
                        confirm_reset_token_billing = false;
                        selected_row = selected_row.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        confirm_reset_token_billing = false;
                        if selected_row + 1 < total_rows {
                            selected_row += 1;
                        }
                    }
                    KeyCode::Enter => {
                        confirm_reset_token_billing = false;
                        let mut app = state.lock().await;
                        if selected_row < themes.len() {
                            let picked = themes[selected_row];
                            if app.theme != picked.id {
                                app.theme = picked.id.to_string();
                                app.log("INFO", format!("Theme changed to {}", picked.label));
                                app.persist_state_with_log();
                            }
                        } else {
                            let tool_mode_start = themes.len();
                            let tool_mode_end = tool_mode_start + tool_modes.len();
                            if selected_row < tool_mode_end {
                                let picked = tool_modes[selected_row - tool_mode_start];
                                if app.tool_mode != picked {
                                    app.tool_mode = picked;
                                    app.log("INFO", format!("Tool mode: {}", picked.label()));
                                    app.persist_state_with_log();
                                }
                            } else {
                                app.set_catdesk_as_co_author = !app.set_catdesk_as_co_author;
                                let enabled = app.set_catdesk_as_co_author;
                                app.log(
                                    "INFO",
                                    format!(
                                        "Set CatDesk as co-author: {}",
                                        if enabled { "enabled" } else { "disabled" }
                                    ),
                                );
                                app.persist_state_with_log();
                            }
                        }
                    }
                    KeyCode::Char('r') => {
                        if !confirm_reset_token_billing {
                            confirm_reset_token_billing = true;
                            continue;
                        }
                        let mut app = state.lock().await;
                        app.usage_totals = UsageTotals::default();
                        app.log("INFO", "Token billing totals reset".into());
                        app.persist_state_with_log();
                        confirm_reset_token_billing = false;
                    }
                    _ => {
                        confirm_reset_token_billing = false;
                    }
                }
            }
        }
    }
}

fn draw_settings(
    f: &mut Frame,
    current_theme: &theme::ThemeDef,
    current_tool_mode: ToolMode,
    set_catdesk_as_co_author: bool,
    usage_totals: &UsageTotals,
    selected_row: usize,
    confirm_reset_token_billing: bool,
) {
    let themes = theme::all();
    let tool_modes = ToolMode::all();
    let palette = current_theme.palette;
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    let header = Paragraph::new("  Settings")
        .style(
            Style::default()
                .fg(palette.header_fg)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(palette.border_type)
                .border_style(Style::default().fg(palette.border_fg)),
        );
    f.render_widget(header, chunks[0]);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Choose a theme:",
            Style::default()
                .fg(palette.title_fg)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    for (idx, theme) in themes.iter().enumerate() {
        let selected = idx == selected_row;
        let marker = if selected { ">" } else { " " };
        let name_style = if selected {
            Style::default()
                .fg(palette.key_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.primary_fg)
        };
        lines.push(Line::from(""));
        let mut spans = vec![Span::styled(
            format!(" {} [{}] {}", marker, idx + 1, theme.label),
            name_style,
        )];
        if theme.id == current_theme.id {
            spans.push(Span::styled(
                "  [current]",
                Style::default()
                    .fg(palette.secondary_fg)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(spans));
        lines.push(Line::from(vec![Span::styled(
            format!("     {}", theme.description),
            Style::default().fg(palette.muted_fg),
        )]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Choose a tool mode:",
        Style::default()
            .fg(palette.title_fg)
            .add_modifier(Modifier::BOLD),
    )]));
    for (idx, tool_mode) in tool_modes.iter().enumerate() {
        let row_idx = themes.len() + idx;
        let selected = row_idx == selected_row;
        let marker = if selected { ">" } else { " " };
        let name_style = if selected {
            Style::default()
                .fg(palette.key_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.primary_fg)
        };
        lines.push(Line::from(""));
        let mut spans = vec![Span::styled(
            format!(" {} [{}] {}", marker, row_idx + 1, tool_mode.label()),
            name_style,
        )];
        if *tool_mode == current_tool_mode {
            spans.push(Span::styled(
                "  [current]",
                Style::default()
                    .fg(palette.secondary_fg)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(spans));
        lines.push(Line::from(vec![Span::styled(
            format!("     {}", tool_mode.description()),
            Style::default().fg(palette.muted_fg),
        )]));
    }
    let co_author_row = themes.len() + tool_modes.len();
    let co_author_selected = co_author_row == selected_row;
    let co_author_marker = if co_author_selected { ">" } else { " " };
    let co_author_name_style = if co_author_selected {
        Style::default()
            .fg(palette.key_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.primary_fg)
    };
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Commit attribution:",
        Style::default()
            .fg(palette.title_fg)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![Span::styled(
        format!(
            " {} [{}] Set CatDesk as co-author",
            co_author_marker,
            co_author_row + 1
        ),
        co_author_name_style,
    )]));
    lines.push(Line::from(vec![
        Span::styled("     ", Style::default()),
        Span::styled(
            if set_catdesk_as_co_author {
                "[enabled]"
            } else {
                "[disabled]"
            },
            Style::default().fg(if set_catdesk_as_co_author {
                palette.success_fg
            } else {
                palette.muted_fg
            }),
        ),
    ]));
    lines.push(Line::from(vec![Span::styled(
        "     When enabled, CatDesk automatically appends \"Co-Authored-By: CatDesk\" to git commits and blocks manually written CatDesk co-author trailers.",
        Style::default().fg(palette.muted_fg),
    )]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Token billing:",
        Style::default()
            .fg(palette.title_fg)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::styled("  Input: ", Style::default().fg(palette.muted_fg)),
        Span::styled(
            usage_totals.input_tokens.to_string(),
            Style::default().fg(palette.primary_fg),
        ),
        Span::styled("   Output: ", Style::default().fg(palette.muted_fg)),
        Span::styled(
            usage_totals.output_tokens.to_string(),
            Style::default().fg(palette.primary_fg),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Total: ", Style::default().fg(palette.muted_fg)),
        Span::styled(
            usage_totals.total_tokens.to_string(),
            Style::default()
                .fg(palette.secondary_fg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   Tool calls: ", Style::default().fg(palette.muted_fg)),
        Span::styled(
            usage_totals.tool_call_count.to_string(),
            Style::default().fg(palette.primary_fg),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  [r]", Style::default().fg(palette.warning_fg)),
        Span::styled(
            if confirm_reset_token_billing {
                " Press again to confirm token billing reset"
            } else {
                " Reset token billing totals"
            },
            Style::default().fg(if confirm_reset_token_billing {
                palette.danger_fg
            } else {
                palette.muted_fg
            }),
        ),
    ]));

    let body = Paragraph::new(lines).block(
        Block::default()
            .title(" Theme, Tool Mode & Billing ")
            .borders(Borders::ALL)
            .border_type(palette.border_type)
            .border_style(Style::default().fg(palette.border_fg)),
    );
    f.render_widget(body, chunks[1]);

    let keys = Paragraph::new(Line::from(vec![
        Span::styled("  [Up/Down]", Style::default().fg(palette.key_fg)),
        Span::raw(" Select  "),
        Span::styled("[Enter]", Style::default().fg(palette.success_fg)),
        Span::raw(" Apply  "),
        Span::styled(
            "[r]",
            Style::default().fg(if confirm_reset_token_billing {
                palette.danger_fg
            } else {
                palette.warning_fg
            }),
        ),
        Span::raw(if confirm_reset_token_billing {
            " Confirm reset  "
        } else {
            " Reset token billing  "
        }),
        Span::styled("[q/Esc]", Style::default().fg(palette.danger_fg)),
        Span::raw(" Back"),
    ]))
    .block(
        Block::default()
            .title(" Keys ")
            .borders(Borders::ALL)
            .border_type(palette.border_type)
            .border_style(Style::default().fg(palette.border_fg)),
    );
    f.render_widget(keys, chunks[2]);
}

async fn mode_is_browser_enabled(state: SharedState) -> bool {
    state.lock().await.mode.browser_enabled()
}

fn browser_identity_matches(
    browser: &browser::DetectedBrowser,
    selected: &browser::DetectedBrowser,
) -> bool {
    browser.path == selected.path && browser.binary == selected.binary
}

fn selected_supported_browser_idx(
    browsers: &[browser::DetectedBrowser],
    selected_browser: Option<&browser::DetectedBrowser>,
) -> usize {
    let supported_indices: Vec<usize> = browsers
        .iter()
        .enumerate()
        .filter(|(_, browser)| browser.mcp_supported)
        .map(|(idx, _)| idx)
        .collect();
    if supported_indices.is_empty() {
        return 0;
    }
    let Some(selected_browser) = selected_browser else {
        return 0;
    };
    let Some(browser_idx) = browsers
        .iter()
        .position(|browser| browser_identity_matches(browser, selected_browser))
    else {
        return 0;
    };
    supported_indices
        .iter()
        .position(|idx| *idx == browser_idx)
        .unwrap_or(0)
}

async fn run_browser_select(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: SharedState,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut browsers = browser::detect_browsers();
    let mut selected_supported_idx = {
        let mut app = state.lock().await;
        app.detected_browsers = browsers.clone();
        let selected_missing = app.selected_browser.as_ref().is_some_and(|selected| {
            !browsers
                .iter()
                .any(|browser| browser_identity_matches(browser, selected))
        });
        if selected_missing {
            app.selected_browser = None;
            app.persist_state_with_log();
        }
        selected_supported_browser_idx(&browsers, app.selected_browser.as_ref())
    };
    loop {
        let supported_indices: Vec<usize> = browsers
            .iter()
            .enumerate()
            .filter(|(_, b)| b.mcp_supported)
            .map(|(idx, _)| idx)
            .collect();
        if !supported_indices.is_empty() {
            selected_supported_idx =
                selected_supported_idx.min(supported_indices.len().saturating_sub(1));
        } else {
            selected_supported_idx = 0;
        }

        let current_theme = {
            let app = state.lock().await;
            app.current_theme()
        };
        terminal.draw(|f| {
            draw_browser_select(
                f,
                &browsers,
                &supported_indices,
                selected_supported_idx,
                current_theme,
            )
        })?;

        if event::poll(UI_POLL_INTERVAL)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') => return Ok(false),
                    KeyCode::Char('r') => {
                        browsers = browser::detect_browsers();
                        let mut app = state.lock().await;
                        app.detected_browsers = browsers.clone();
                        let selected_missing =
                            app.selected_browser.as_ref().is_some_and(|selected| {
                                !browsers
                                    .iter()
                                    .any(|browser| browser_identity_matches(browser, selected))
                            });
                        if selected_missing {
                            app.selected_browser = None;
                            app.persist_state_with_log();
                        }
                        selected_supported_idx = selected_supported_browser_idx(
                            &browsers,
                            app.selected_browser.as_ref(),
                        );
                    }
                    KeyCode::Up => {
                        selected_supported_idx = selected_supported_idx.saturating_sub(1)
                    }
                    KeyCode::Down => {
                        if selected_supported_idx + 1 < supported_indices.len() {
                            selected_supported_idx += 1;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(selected_idx) = supported_indices.get(selected_supported_idx) {
                            if let Some(selected) = browsers.get(*selected_idx).cloned() {
                                persist_selected_browser(state.clone(), selected).await;
                                return Ok(true);
                            }
                        }
                    }
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        let index = c.to_digit(10).unwrap_or(0) as usize;
                        if index == 0 {
                            continue;
                        }
                        let target_idx = index - 1;
                        if let Some(browser_idx) = supported_indices.get(target_idx) {
                            if let Some(selected) = browsers.get(*browser_idx).cloned() {
                                persist_selected_browser(state.clone(), selected).await;
                                return Ok(true);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn persist_selected_browser(state: SharedState, selected: browser::DetectedBrowser) {
    let remote_info = selected
        .remote_debug_target
        .as_deref()
        .unwrap_or("not active");
    let mut app = state.lock().await;
    app.selected_browser = Some(selected.clone());
    app.log(
        "INFO",
        format!(
            "Selected browser: {} ({}, {})",
            selected.name, selected.binary, selected.path
        ),
    );
    app.log(
        "INFO",
        format!("Selected browser remote debugging: {remote_info}"),
    );
    app.persist_state_with_log();
}

fn draw_browser_select(
    f: &mut Frame,
    browsers: &[browser::DetectedBrowser],
    supported_indices: &[usize],
    selected_supported_idx: usize,
    theme: &theme::ThemeDef,
) {
    let palette = theme.palette;
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

    let header = Paragraph::new("  Select Browser - Installed and Remote Debugging Status")
        .style(
            Style::default()
                .fg(palette.header_fg)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(palette.border_type)
                .border_style(Style::default().fg(palette.border_fg)),
        );
    f.render_widget(header, chunks[0]);

    let active_summary = browser::format_active_remote_debug_names(browsers);
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(
                "  Installed browsers: ",
                Style::default().fg(palette.muted_fg),
            ),
            Span::styled(
                browsers.len().to_string(),
                Style::default()
                    .fg(palette.title_fg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Remote debugging active: ",
                Style::default().fg(palette.muted_fg),
            ),
            Span::styled(active_summary, Style::default().fg(palette.success_fg)),
        ]),
        Line::from(vec![
            Span::styled(
                "  Selectable (Chromium): ",
                Style::default().fg(palette.muted_fg),
            ),
            Span::styled(
                supported_indices.len().to_string(),
                Style::default()
                    .fg(palette.key_fg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
    ];

    if browsers.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No browser found in PATH. Press [r] to rescan, [q] to quit.",
            Style::default().fg(palette.danger_fg),
        )));
    } else if supported_indices.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Only unsupported browsers found (e.g. Firefox). Chromium browsers are required.",
            Style::default().fg(palette.danger_fg),
        )));
        lines.push(Line::from(""));
        for browser in browsers {
            lines.push(Line::from(vec![Span::styled(
                format!("   [x] {} ({})", browser.name, browser.binary),
                Style::default().fg(palette.muted_fg),
            )]));
            lines.push(Line::from(vec![Span::styled(
                format!("     status: {}", browser.support_note),
                Style::default().fg(palette.warning_fg),
            )]));
            lines.push(Line::from(""));
        }
    } else {
        let selected_browser_index = supported_indices
            .get(selected_supported_idx)
            .copied()
            .unwrap_or(supported_indices[0]);
        for (idx, browser) in browsers.iter().enumerate() {
            let selected = idx == selected_browser_index;
            let prefix = if selected { ">" } else { " " };
            let quick_pick_num = supported_indices
                .iter()
                .position(|candidate_idx| *candidate_idx == idx)
                .map(|v| v + 1);
            let title_style = if !browser.mcp_supported {
                Style::default().fg(palette.muted_fg)
            } else if selected {
                Style::default()
                    .fg(palette.key_fg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.primary_fg)
            };
            if let Some(num) = quick_pick_num {
                lines.push(Line::from(vec![Span::styled(
                    format!(
                        " {} [{}] {} ({})",
                        prefix, num, browser.name, browser.binary
                    ),
                    title_style,
                )]));
            } else {
                lines.push(Line::from(vec![Span::styled(
                    format!("   [x] {} ({})", browser.name, browser.binary),
                    title_style,
                )]));
            }
            lines.push(Line::from(vec![Span::styled(
                format!("     path: {}", browser.path),
                Style::default().fg(palette.muted_fg),
            )]));
            lines.push(Line::from(vec![Span::styled(
                format!("     status: {}", browser.support_note),
                Style::default().fg(if browser.mcp_supported {
                    palette.success_fg
                } else {
                    palette.warning_fg
                }),
            )]));
            if !browser.mcp_supported {
                lines.push(Line::from(vec![Span::styled(
                    "     remote debugging integration: not supported yet",
                    Style::default().fg(palette.warning_fg),
                )]));
            } else if browser.remote_debug_active {
                let target = browser.remote_debug_target.as_deref().unwrap_or("unknown");
                let pid = browser
                    .remote_debug_pid
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "--".into());
                lines.push(Line::from(vec![Span::styled(
                    format!("     remote debugging: ACTIVE at {target} (pid {pid})"),
                    Style::default().fg(palette.success_fg),
                )]));
            } else {
                lines.push(Line::from(vec![Span::styled(
                    format!(
                        "     remote debugging: not active (supported flag: {})",
                        browser.remote_debug_hint
                    ),
                    Style::default().fg(palette.warning_fg),
                )]));
            }
            lines.push(Line::from(""));
        }
    }

    let body = Paragraph::new(lines).block(
        Block::default()
            .title(" Browser List ")
            .borders(Borders::ALL)
            .border_type(palette.border_type)
            .border_style(Style::default().fg(palette.border_fg)),
    );
    f.render_widget(body, chunks[1]);

    let keys = Paragraph::new(Line::from(vec![
        Span::styled("  [Up/Down]", Style::default().fg(palette.key_fg)),
        Span::raw(" Select  "),
        Span::styled("[1-9]", Style::default().fg(palette.key_fg)),
        Span::raw(" Quick select (Chromium only)  "),
        Span::styled("[Enter]", Style::default().fg(palette.success_fg)),
        Span::raw(" Confirm  "),
        Span::styled("[r]", Style::default().fg(palette.warning_fg)),
        Span::raw(" Rescan  "),
        Span::styled("[q]", Style::default().fg(palette.danger_fg)),
        Span::raw(" Quit"),
    ]))
    .block(
        Block::default()
            .title(" Keys ")
            .borders(Borders::ALL)
            .border_type(palette.border_type)
            .border_style(Style::default().fg(palette.border_fg)),
    );
    f.render_widget(keys, chunks[2]);
}

fn find_available_remote_debug_port(start: u16, end: u16) -> Option<u16> {
    (start..=end).find(|port| std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok())
}

fn sanitize_for_filename(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if sanitized.is_empty() {
        "browser".into()
    } else {
        sanitized
    }
}

async fn wait_remote_debug_ready(port: u16, timeout: Duration) -> bool {
    let client = reqwest::Client::new();
    let endpoint = format!("http://127.0.0.1:{port}/json/version");
    let started = Instant::now();
    while started.elapsed() < timeout {
        let result = client
            .get(&endpoint)
            .timeout(Duration::from_millis(600))
            .send()
            .await;
        if let Ok(response) = result {
            if response.status().is_success() {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    false
}

async fn ensure_selected_browser_remote_debugging(
    state: SharedState,
    selected_browser: Option<browser::DetectedBrowser>,
) -> Option<browser::DetectedBrowser> {
    let Some(mut selected) = selected_browser else {
        return None;
    };
    if !selected.mcp_supported {
        state.lock().await.log(
            "ERROR",
            format!(
                "Selected browser {} is not supported yet for chrome-devtools-mcp",
                selected.name
            ),
        );
        return None;
    }
    if selected.remote_debug_active && selected.remote_debug_target.is_some() {
        return Some(selected);
    }

    let Some(port) = find_available_remote_debug_port(9222, 9322) else {
        state.lock().await.log(
            "ERROR",
            "No available local port in range 9222-9322 for remote debugging".into(),
        );
        return Some(selected);
    };

    let user_data_dir = format!(
        "/tmp/catdesk-remote-debug-{}",
        sanitize_for_filename(&selected.binary)
    );
    if let Err(e) = std::fs::create_dir_all(&user_data_dir) {
        state.lock().await.log(
            "WARN",
            format!("Failed to create user data dir {user_data_dir}: {e}"),
        );
    }

    let mut command = tokio::process::Command::new(&selected.path);
    command
        .arg(format!("--remote-debugging-port={port}"))
        .arg("--remote-debugging-address=127.0.0.1")
        .arg(format!("--user-data-dir={user_data_dir}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            state.lock().await.log(
                "ERROR",
                format!(
                    "Failed to launch {} with remote debugging: {}",
                    selected.name, e
                ),
            );
            return Some(selected);
        }
    };
    let launched_pid = child.id();

    let existing_child = {
        let mut app = state.lock().await;
        app.remote_browser_child.take()
    };
    if let Some(mut old_child) = existing_child {
        let _ = old_child.kill().await;
    }

    {
        let mut app = state.lock().await;
        app.remote_browser_child = Some(child);
        app.log(
            "INFO",
            format!(
                "Launched {} with remote debugging on 127.0.0.1:{}",
                selected.name, port
            ),
        );
    }

    if wait_remote_debug_ready(port, Duration::from_secs(10)).await {
        selected.remote_debug_active = true;
        selected.remote_debug_target = Some(format!("127.0.0.1:{port}"));
        selected.remote_debug_pid = launched_pid;
        {
            let mut app = state.lock().await;
            app.selected_browser = Some(selected.clone());
            app.log(
                "INFO",
                format!(
                    "Remote debugging ready for {} at 127.0.0.1:{}",
                    selected.name, port
                ),
            );
            app.persist_state_with_log();
        }
        Some(selected)
    } else {
        state.lock().await.log(
            "WARN",
            format!(
                "Remote debugging endpoint for {} did not become ready in time",
                selected.name
            ),
        );
        Some(selected)
    }
}

// ── Start services ──────────────────────────────────────────

async fn start_services(
    state: SharedState,
    ui_events: UnboundedSender<ServerUiEvent>,
) -> Option<Arc<Mutex<DevtoolsBridge>>> {
    let (mode, mut detected_browsers, mut selected_browser) = {
        let app = state.lock().await;
        (
            app.mode,
            app.detected_browsers.clone(),
            app.selected_browser.clone(),
        )
    };

    if mode.browser_enabled() && detected_browsers.is_empty() {
        detected_browsers = browser::detect_browsers();
    }
    if mode.browser_enabled() {
        selected_browser =
            ensure_selected_browser_remote_debugging(state.clone(), selected_browser).await;
        detected_browsers = browser::detect_browsers();
        if let Some(selected) = &selected_browser {
            if let Some(refreshed) = detected_browsers
                .iter()
                .find(|b| b.path == selected.path && b.binary == selected.binary)
                .cloned()
            {
                selected_browser = Some(refreshed);
            }
        }
        let mut app = state.lock().await;
        app.detected_browsers = detected_browsers.clone();
        app.selected_browser = selected_browser.clone();
        app.persist_state_with_log();
    }

    let browser_summary = browser::format_browser_names(&detected_browsers);
    let remote_support_summary = browser::format_remote_debug_names(&detected_browsers);
    let remote_active_summary = browser::format_active_remote_debug_names(&detected_browsers);
    let browser_details: Vec<String> = detected_browsers
        .iter()
        .map(|b| {
            format!(
                "Browser: {} (binary: {}, path: {}, support: {}, remote debug flag: {}, remote debug active: {}, pid: {})",
                b.name,
                b.binary,
                b.path,
                b.support_note,
                b.remote_debug_hint,
                b.remote_debug_target.as_deref().unwrap_or("no"),
                b.remote_debug_pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "--".into())
            )
        })
        .collect();
    {
        let mut app = state.lock().await;
        app.detected_browsers = detected_browsers;
        if browser_summary == "--" {
            app.log("WARN", "No local browser found in PATH".into());
        } else {
            app.log("INFO", format!("Local browsers: {browser_summary}"));
        }
        if remote_support_summary == "--" {
            app.log(
                "WARN",
                "No detected browser supports remote debugging".into(),
            );
        } else {
            app.log(
                "INFO",
                format!("Remote debugging supported: {remote_support_summary}"),
            );
        }
        if remote_active_summary == "--" {
            app.log(
                "WARN",
                "No browser currently runs with remote debugging".into(),
            );
        } else {
            app.log(
                "INFO",
                format!("Remote debugging active: {remote_active_summary}"),
            );
        }
        if mode.browser_enabled() {
            if let Some(selected) = &selected_browser {
                let target = selected
                    .remote_debug_target
                    .as_deref()
                    .unwrap_or("launch new browser instance");
                app.log(
                    "INFO",
                    format!(
                        "Using browser: {} ({}) -> {}",
                        selected.name, selected.path, target
                    ),
                );
            } else {
                app.log("WARN", "No browser was selected before startup".into());
            }
        }
        for detail in browser_details {
            app.log("INFO", detail);
        }
    }

    // Start MCP HTTP server
    let devtools_bridge = if mode.browser_enabled() {
        if selected_browser.is_none() {
            state.lock().await.log(
                "ERROR",
                "Browser mode requires selecting a supported Chromium browser".into(),
            );
            None
        } else {
            state
                .lock()
                .await
                .log("INFO", "Starting chrome-devtools-mcp...".into());
            match DevtoolsBridge::start(selected_browser.as_ref()).await {
                Ok(bridge) => {
                    let mut app = state.lock().await;
                    app.devtools_running = true;
                    app.log("INFO", "chrome-devtools-mcp started".into());
                    Some(bridge)
                }
                Err(e) => {
                    let mut app = state.lock().await;
                    app.log("ERROR", format!("chrome-devtools-mcp: {e}"));
                    None
                }
            }
        }
    } else {
        None
    };

    let (mcp_path, bind_host, bind_port) = {
        let app = state.lock().await;
        (app.mcp_path(), app.mcp_bind_host.clone(), app.port)
    };
    let router = server::router(
        state.clone(),
        devtools_bridge.clone(),
        mcp_path.clone(),
        ui_events,
        None,
    );
    let bind_identity = format!("{bind_host}:{bind_port}");
    let listener = match tokio::net::TcpListener::bind(&bind_identity).await {
        Ok(l) => l,
        Err(e) => {
            state.lock().await.log(
                "ERROR",
                format!("Failed to bind MCP listener {bind_identity}: {e}"),
            );
            return devtools_bridge;
        }
    };
    if let Err(error) = daemon_reload::make_listener_non_inheritable(&listener) {
        state.lock().await.log(
            "ERROR",
            format!("Failed to secure MCP listener inheritance: {error}"),
        );
        return devtools_bridge;
    }
    let local_addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(e) => {
            state.lock().await.log(
                "ERROR",
                format!("Failed to inspect MCP listener address: {e}"),
            );
            return devtools_bridge;
        }
    };
    if !local_addr.ip().is_loopback()
        && crate::tunnel::requires_loopback(state.lock().await.tunnel_config.mode)
    {
        state.lock().await.log(
            "ERROR",
            format!(
                "Refusing private transport MCP listener on non-loopback address: {local_addr}"
            ),
        );
        return devtools_bridge;
    }

    // The fixed control-pipe client is deliberately reachable only after the
    // production listener has bound, had inheritance cleared, and proved its
    // compiled 127.0.0.1:3200 identity. A missing/refusing supervisor leaves
    // this local worker untouched; it never writes supervisor state directly.
    #[cfg(windows)]
    if is_canonical_production_worker_listener(&local_addr) {
        let registered = match crate::reviewed_build::verified_current_reviewed_main_image_digest()
        {
            Ok(manifest) => {
                crate::windows_supervisor_control_pipe::register_fixed_ready_worker_listener(
                    &listener, &manifest,
                )
                .await
            }
            Err(_) => Err(
                crate::control_plane_supervisor::ControlPlaneSupervisorError::InvalidRegistration,
            ),
        };
        if registered.is_err() {
            state.lock().await.log(
                "WARN",
                "Fixed control-plane supervisor registration evidence refused or unavailable"
                    .into(),
            );
        }
    }

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    {
        let mut app = state.lock().await;
        app.server_running = true;
        app.server_handle = Some(handle);
        app.port = local_addr.port();
        app.log(
            "INFO",
            format!("MCP Server started on {}", safe_bind_identity(&local_addr)),
        );
    }

    // Start ngrok
    if let Err(e) = ngrok::start_transport(state.clone()).await {
        state.lock().await.log("ERROR", format!("transport: {e}"));
    }
    refresh_transport_health(state.clone()).await;

    // Reviewer/wake ownership is process-local, while WAITING_FOR_CHATGPT and
    // its actionable review are durable. Recreate only that out-of-band wake
    // responsibility after services are healthy; never resume provider work
    // from startup here.
    let wake_workspace = {
        let app = state.lock().await;
        PathBuf::from(&app.workspace_root)
    };
    match delegated::autonomy_runtime::rehydrate_persisted_waiting_wakes(&wake_workspace) {
        Ok(count) if count > 0 => state.lock().await.log(
            "INFO",
            format!("Rehydrated {count} persisted ChatGPT wake handoff(s)"),
        ),
        Ok(_) => {}
        Err(_) => state.lock().await.log(
            "WARN",
            "Persisted ChatGPT wake handoff rehydration failed closed".into(),
        ),
    }

    operator_facade::start_web_auto_scheduler(state.clone());

    devtools_bridge
}

// ── Phase 2: Main TUI ──────────────────────────────────────

async fn refresh_transport_health(state: SharedState) {
    crate::ngrok::refresh_openai_child_exit_status(&state).await;
    let (
        bind_host,
        port,
        mcp_path,
        remote_check_enabled,
        remote_mcp_url,
        tunnel_mode,
        current_health,
    ) = {
        let app = state.lock().await;
        (
            app.mcp_bind_host.clone(),
            app.port,
            app.mcp_path(),
            app.tunnel_config.remote_self_check,
            app.public_mcp_url(),
            app.tunnel_config.mode,
            app.transport_health.clone(),
        )
    };
    let local_endpoint = format!("{}{}", loopback_http_origin(&bind_host, port), mcp_path);
    let mut snapshot =
        crate::tunnel::TransportHealthSnapshot::configured_unverified(remote_check_enabled);
    snapshot.last_checked_at = Some(crate::tunnel::current_startup_time());

    match crate::tunnel::run_mcp_endpoint_self_check(
        &local_endpoint,
        None,
        crate::tunnel::MCP_SELF_CHECK_TIMEOUT,
        false,
    )
    .await
    {
        Ok(report) => {
            snapshot.local_mcp = "READY".into();
            if remote_check_enabled {
                if let Some(remote_endpoint) = remote_mcp_url.as_deref() {
                    match crate::tunnel::run_mcp_endpoint_self_check(
                        remote_endpoint,
                        None,
                        crate::tunnel::MCP_SELF_CHECK_TIMEOUT,
                        true,
                    )
                    .await
                    {
                        Ok(_) => {
                            snapshot.health = crate::tunnel::TransportHealth::ConnectedVerified;
                        }
                        Err(error) => {
                            snapshot.health = crate::tunnel::TransportHealth::Degraded;
                            snapshot.redacted_reason = Some(error);
                            snapshot.warnings.push(
                                "Remote MCP self-check failed; no fallback transport was started"
                                    .into(),
                            );
                        }
                    }
                } else {
                    snapshot.health = crate::tunnel::TransportHealth::Disconnected;
                    snapshot.redacted_reason = Some(
                        "remote self-check enabled but no public endpoint is configured".into(),
                    );
                }
            } else {
                snapshot.health =
                    if matches!(tunnel_mode, crate::tunnel::TunnelMode::OpenaiSecureTunnel) {
                        match current_health.health {
                            crate::tunnel::TransportHealth::Connecting
                            | crate::tunnel::TransportHealth::ConnectedVerified
                            | crate::tunnel::TransportHealth::Disconnected
                            | crate::tunnel::TransportHealth::Failed => current_health.health,
                            _ => crate::tunnel::TransportHealth::ConfiguredUnverified,
                        }
                    } else {
                        crate::tunnel::TransportHealth::LocalReady
                    };
                if matches!(tunnel_mode, crate::tunnel::TunnelMode::OpenaiSecureTunnel) {
                    snapshot.redacted_reason = current_health.redacted_reason.clone();
                    snapshot.warnings.extend(current_health.warnings.clone());
                }
            }
            snapshot.warnings.push(format!(
                "Local MCP self-check passed with {} tools; endpoint fingerprint {}",
                report.tool_count, report.endpoint_fingerprint
            ));
        }
        Err(error) => {
            snapshot.health =
                if matches!(tunnel_mode, crate::tunnel::TunnelMode::OpenaiSecureTunnel) {
                    crate::tunnel::TransportHealth::BlockedLocalMcpUnavailable
                } else {
                    crate::tunnel::TransportHealth::Failed
                };
            snapshot.local_mcp = "FAILED".into();
            snapshot.redacted_reason = Some(error);
        }
    }

    let mut app = state.lock().await;
    let health = snapshot.health.as_str();
    app.transport_health = snapshot;
    app.log("INFO", format!("Transport health: {health}"));
}

/// Publishes a UI-only autonomy snapshot after bounded local reads. No AppState
/// lock is held while accessing durable state, and no MCP self-call is made.
async fn refresh_autonomy_observability(state: SharedState) {
    let (workspace, transport) = {
        let app = state.lock().await;
        (
            PathBuf::from(&app.workspace_root),
            format!(
                "{} / {}",
                app.transport_health.health.as_str(),
                app.transport_health.local_mcp
            ),
        )
    };
    let fallback_transport = transport.clone();
    let snapshot =
        tokio::task::spawn_blocking(move || read_autonomy_snapshot(&workspace, &transport))
            .await
            .unwrap_or_else(|_| {
                crate::delegated::autonomy_observability::AutonomyObservabilitySnapshotV1::unknown(
                    &fallback_transport,
                    "autonomy refresh failed",
                )
            });
    state.lock().await.autonomy_observability = snapshot;
}

fn is_canonical_production_worker_listener(address: &std::net::SocketAddr) -> bool {
    address.ip() == std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        && address.port() == crate::control_plane_supervisor::FIXED_LOCAL_BACKEND_PORT
}

fn loopback_http_origin(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("http://[{host}]:{port}")
    } else {
        format!("http://{host}:{port}")
    }
}

fn safe_bind_identity(addr: &std::net::SocketAddr) -> String {
    if addr.ip().is_loopback() {
        format!("loopback:{}", addr.port())
    } else {
        format!("non-loopback:{}", addr.port())
    }
}

async fn run_tui(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: SharedState,
    _devtools: Option<Arc<Mutex<DevtoolsBridge>>>,
    mut ui_events: UnboundedReceiver<ServerUiEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut log_scroll: usize = 0;
    let mut log_follow_tail = true;
    let mut last_log_max_scroll: usize = 0;
    let mut last_log_effective_scroll: usize = 0;
    let mut selection = Selection::new();
    // (message, position (col, row), created_at)
    let mut toast: Option<(&str, (u16, u16), Instant)> = None;
    #[allow(unused_assignments)]
    let mut screen_lines: Vec<String> = vec![];
    let mut last_animation_snapshot = String::new();
    #[allow(unused_assignments)]
    let mut last_mcp_url: Option<String> = None;
    let mut last_autonomy_refresh = Instant::now()
        .checked_sub(AUTONOMY_REFRESH_INTERVAL)
        .unwrap_or_else(Instant::now);

    loop {
        if last_autonomy_refresh.elapsed() >= AUTONOMY_REFRESH_INTERVAL {
            refresh_autonomy_observability(state.clone()).await;
            last_autonomy_refresh = Instant::now();
        }
        {
            let mut app = state.lock().await;
            drain_server_ui_events(&mut app, &mut ui_events);
            app.prune_closed_flows();
        }
        {
            let app = state.lock().await;
            last_mcp_url = app.public_mcp_url();
            let toast_ref = toast
                .as_ref()
                .filter(|(_, _, t)| t.elapsed().as_secs() < 2)
                .map(|(m, pos, _)| (*m, *pos));
            let mut new_lines: Vec<String> = Vec::new();
            let mut latest_log_view: Option<(usize, usize)> = None;
            terminal.draw(|f| {
                draw_ui(
                    f,
                    &app,
                    log_scroll,
                    log_follow_tail,
                    &mut latest_log_view,
                    toast_ref,
                );

                if let Some(((c0, r0), (c1, r1))) = selection.range() {
                    let palette = app.current_theme().palette;
                    let area = f.area();
                    for row in r0..=r1 {
                        if row >= area.height {
                            break;
                        }
                        let cs = if row == r0 { c0 } else { 0 };
                        let ce = if row == r1 {
                            c1
                        } else {
                            area.width.saturating_sub(1)
                        };
                        for col in cs..=ce {
                            if col >= area.width {
                                break;
                            }
                            if let Some(cell) = f.buffer_mut().cell_mut((col, row)) {
                                cell.set_style(
                                    Style::default()
                                        .bg(palette.selection_bg)
                                        .fg(palette.selection_fg),
                                );
                            }
                        }
                    }
                }

                let area = f.area();
                let buf = f.buffer_mut();
                for row in 0..area.height {
                    let mut line = String::new();
                    for col in 0..area.width {
                        line.push_str(buf[(col, row)].symbol());
                    }
                    new_lines.push(line);
                }
            })?;
            if let Some((max_scroll, effective_scroll)) = latest_log_view {
                last_log_max_scroll = max_scroll;
                last_log_effective_scroll = effective_scroll;
                if !log_follow_tail && log_scroll > last_log_max_scroll {
                    log_scroll = last_log_max_scroll;
                }
            }
            screen_lines = new_lines;
        }

        let snapshots = {
            let app = state.lock().await;
            build_animation_snapshot(&app)
        };
        if !snapshots.is_empty() {
            let snapshot_joined = snapshots.join("\n");
            if snapshot_joined != last_animation_snapshot {
                last_animation_snapshot = snapshot_joined;
            }
        }

        if let Some((_, _, t)) = &toast {
            if t.elapsed().as_secs() >= 2 {
                toast = None;
            }
        }

        if event::poll(UI_POLL_INTERVAL)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    selection.clear();
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Up => {
                            if log_follow_tail {
                                log_follow_tail = false;
                                log_scroll = last_log_effective_scroll.saturating_sub(1);
                            } else {
                                log_scroll = log_scroll.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            if !log_follow_tail {
                                log_scroll = (log_scroll + 1).min(last_log_max_scroll);
                                if log_scroll >= last_log_max_scroll {
                                    log_follow_tail = true;
                                }
                            }
                        }
                        KeyCode::End => {
                            log_follow_tail = true;
                            log_scroll = last_log_max_scroll;
                        }
                        KeyCode::Char('h') => {
                            refresh_transport_health(state.clone()).await;
                            toast = Some(("Transport health checked", (2, 2), Instant::now()));
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        selection.start = Some((mouse.column, mouse.row));
                        selection.end = Some((mouse.column, mouse.row));
                        selection.dragging = true;
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        if selection.dragging {
                            selection.end = Some((mouse.column, mouse.row));
                        }
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        if selection.dragging {
                            selection.end = Some((mouse.column, mouse.row));
                            selection.dragging = false;
                            if let Some((start, end)) = selection.range() {
                                if start != end {
                                    let text = extract_from_screen(&screen_lines, start, end);
                                    if !text.is_empty() {
                                        let message = if clipboard_copy(&text) {
                                            "Copied!"
                                        } else {
                                            "Copy failed"
                                        };
                                        toast = Some((
                                            message,
                                            (mouse.column, mouse.row),
                                            Instant::now(),
                                        ));
                                    }
                                } else {
                                    let row = start.1 as usize;
                                    if row < screen_lines.len() {
                                        let line = &screen_lines[row];
                                        let copy_value = if line.contains("chatgpt.com/apps") {
                                            Some(
                                                "https://chatgpt.com/apps#settings/Connectors"
                                                    .to_string(),
                                            )
                                        } else if let Some(ref url) = last_mcp_url {
                                            let prefix = &url[..url.len().min(30)];
                                            if line.contains("MCP Server URL")
                                                || line.contains(prefix)
                                            {
                                                Some(url.clone())
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                        .or_else(|| {
                                            if line.contains("\u{2502}") {
                                                if line.contains("Name") {
                                                    Some("CatDesk".to_string())
                                                } else if line.contains("Authentication") {
                                                    Some("None".to_string())
                                                } else {
                                                    None
                                                }
                                            } else {
                                                None
                                            }
                                        });
                                        if let Some(text) = copy_value {
                                            let message = if clipboard_copy(&text) {
                                                "Copied!"
                                            } else {
                                                "Copy failed"
                                            };
                                            toast = Some((
                                                message,
                                                (mouse.column, mouse.row),
                                                Instant::now(),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        if log_follow_tail {
                            log_follow_tail = false;
                            log_scroll = last_log_effective_scroll.saturating_sub(1);
                        } else {
                            log_scroll = log_scroll.saturating_sub(1);
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        if !log_follow_tail {
                            log_scroll = (log_scroll + 1).min(last_log_max_scroll);
                            if log_scroll >= last_log_max_scroll {
                                log_follow_tail = true;
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    Ok(())
}

// ── Draw main UI ────────────────────────────────────────────

fn draw_ui(
    f: &mut Frame,
    app: &AppState,
    log_scroll: usize,
    log_follow_tail: bool,
    log_view: &mut Option<(usize, usize)>,
    toast: Option<(&str, (u16, u16))>,
) {
    let palette = app.current_theme().palette;
    let area = f.area();
    let now_millis = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let has_url = app.ngrok_url.is_some();
    let visible_flow_count = app
        .flows
        .iter()
        .filter(|flow| should_display_flow_row(flow, app.remote_connected))
        .count() as u16;
    let show_guide = should_show_connect_guide(app, now_millis);
    let show_flow_panel = !show_guide;
    let bootstrap_status_flow = active_bootstrap_status_flow(app, now_millis);
    let logs_min_height = if show_guide { 3 } else { 5 };
    let max_status_height = area.height.saturating_sub(6 + logs_min_height).max(17);
    // Keep the main panel deterministic: mascot size must not drive layout.
    let status_height = STATUS_PANEL_HEIGHT.min(max_status_height);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(status_height),
            Constraint::Length(3),
            Constraint::Min(logs_min_height),
        ])
        .split(area);

    // ── Header ──
    let header = Paragraph::new("  CatDesk - Turns ChatGPT Web into a coding agent =w=")
        .style(
            Style::default()
                .fg(palette.header_fg)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(palette.border_type)
                .border_style(Style::default().fg(palette.border_fg)),
        );
    f.render_widget(header, chunks[0]);

    // ── Status ──
    let mode_label = app.mode.label();
    let tool_mode_label = app.tool_mode.label();
    let server_status = if app.server_running {
        format!("RUNNING (port {})", app.port)
    } else {
        "STOPPED".into()
    };
    let ngrok_status: &str = if app.ngrok_running {
        "RUNNING"
    } else {
        "STOPPED"
    };
    let devtools_status: &str = if app.devtools_running {
        "RUNNING"
    } else {
        if app.mode.browser_enabled() {
            "STOPPED"
        } else {
            "N/A"
        }
    };
    let mcp_url: String = app.public_mcp_url().unwrap_or_else(|| "--".into());
    let browser_summary = browser::format_browser_names(&app.detected_browsers);
    let remote_support_summary = browser::format_remote_debug_names(&app.detected_browsers);
    let remote_active_summary = browser::format_active_remote_debug_names(&app.detected_browsers);
    let selected_browser_summary = app
        .selected_browser
        .as_ref()
        .map(|b| format!("{} ({})", b.name, b.binary))
        .unwrap_or_else(|| "--".into());
    let selected_target_summary = app
        .selected_browser
        .as_ref()
        .map(|b| {
            b.remote_debug_target
                .clone()
                .unwrap_or_else(|| "launch new browser instance".into())
        })
        .unwrap_or_else(|| "--".into());
    let computer_role_style = Style::default()
        .fg(if app.server_running {
            palette.success_fg
        } else {
            palette.muted_fg
        })
        .add_modifier(Modifier::BOLD);
    let chatgpt_role_style = Style::default()
        .fg(if app.remote_connected {
            palette.success_fg
        } else {
            palette.muted_fg
        })
        .add_modifier(Modifier::BOLD);
    let flow_meta_style = Style::default()
        .fg(palette.info_fg)
        .add_modifier(Modifier::BOLD);
    let lane_for = |active: bool, flow: Option<&FlowLane>| -> Vec<Span<'static>> {
        flow_lane_spans(active, flow, &palette, now_millis)
    };
    let request_stats_for = |app: &AppState| -> Vec<Span<'static>> {
        vec![
            Span::styled("  Requests: ", Style::default().fg(palette.muted_fg)),
            Span::styled(
                app.request_count.to_string(),
                Style::default().fg(palette.title_fg),
            ),
        ]
    };
    let status_label_style = Style::default()
        .fg(palette.primary_fg)
        .add_modifier(Modifier::BOLD);
    let status_label = |label: &'static str| -> Span<'static> {
        Span::styled(
            format!("  {label:<width$} ", width = STATUS_LABEL_WIDTH),
            status_label_style,
        )
    };
    let status_content_height = status_height.saturating_sub(4) as usize;
    let flow_block_lines = 2;

    let mut status_lines: Vec<Line> = vec![
        Line::from(vec![
            status_label("Mode:"),
            Span::styled(
                mode_label,
                Style::default()
                    .fg(palette.secondary_fg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            status_label("Tool mode:"),
            Span::styled(
                tool_mode_label,
                Style::default()
                    .fg(palette.secondary_fg)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            status_label("Server:"),
            Span::styled(
                &server_status,
                Style::default().fg(if app.server_running {
                    palette.success_fg
                } else {
                    palette.danger_fg
                }),
            ),
        ]),
        Line::from(vec![
            status_label("ngrok:"),
            Span::styled(
                ngrok_status,
                Style::default().fg(if app.ngrok_running {
                    palette.success_fg
                } else {
                    palette.danger_fg
                }),
            ),
        ]),
        Line::from(vec![
            status_label("Transport:"),
            Span::styled(
                format!(
                    "{} / {}",
                    app.transport_health.health.as_str(),
                    app.transport_health.local_mcp
                ),
                Style::default().fg(match app.transport_health.health {
                    crate::tunnel::TransportHealth::ConnectedVerified
                    | crate::tunnel::TransportHealth::LocalReady => palette.success_fg,
                    crate::tunnel::TransportHealth::ConfiguredUnverified
                    | crate::tunnel::TransportHealth::Connecting => palette.info_fg,
                    crate::tunnel::TransportHealth::Disabled => palette.muted_fg,
                    _ => palette.danger_fg,
                }),
            ),
        ]),
        Line::from(vec![
            status_label("DevTools:"),
            Span::styled(
                devtools_status,
                Style::default().fg(if app.devtools_running {
                    palette.success_fg
                } else {
                    palette.muted_fg
                }),
            ),
        ]),
        Line::from(vec![
            status_label("MCP Server URL:"),
            Span::styled(
                &mcp_url,
                Style::default().fg(if has_url {
                    palette.info_fg
                } else {
                    palette.muted_fg
                }),
            ),
        ]),
        Line::from(vec![
            status_label("Workspace:"),
            Span::styled(
                &*app.workspace_root,
                Style::default().fg(palette.secondary_fg),
            ),
        ]),
        {
            let mut spans = vec![status_label("Remote connected:")];
            if app.remote_connected {
                spans.push(Span::styled(
                    "V",
                    Style::default()
                        .fg(palette.success_fg)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(
                    "X",
                    Style::default()
                        .fg(palette.danger_fg)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            Line::from(spans)
        },
        session_usage_line(
            &app.session_usage_totals,
            status_label("Session:"),
            &palette,
        ),
    ];

    // This is deliberately distinct from the MCP flow animation above: the
    // flow only visualizes connector traffic, while this panel consumes the
    // redacted, authoritative autonomy snapshot.
    let autonomy = &app.autonomy_observability;
    let autonomy_state = if autonomy.stale {
        format!("{} (STALE)", autonomy.overall.label())
    } else {
        autonomy.overall.label().into()
    };
    status_lines.extend([
        Line::from(vec![
            status_label("Autonomy:"),
            Span::styled(
                autonomy_state,
                Style::default()
                    .fg(palette.info_fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  Actor: "),
            Span::styled(
                autonomy.actor.label(),
                Style::default().fg(palette.secondary_fg),
            ),
        ]),
        Line::from(vec![
            status_label("Ticket / session:"),
            Span::styled(
                trim_line(
                    &format!(
                        "{} / {} / {}",
                        autonomy.project_id, autonomy.task_id, autonomy.session_id
                    ),
                    72,
                ),
                Style::default().fg(palette.title_fg),
            ),
        ]),
        Line::from(vec![
            status_label("Model / reasoning:"),
            Span::styled(
                trim_line(&format!("{} / {}", autonomy.model, autonomy.reasoning), 72),
                Style::default().fg(palette.secondary_fg),
            ),
        ]),
        Line::from(vec![
            status_label("Time:"),
            Span::styled(
                format!(
                    "active {} | wall {} | waiting {} | evidence {}",
                    compact_duration(autonomy.timing.known_active_millis),
                    compact_duration(autonomy.timing.wall_millis),
                    compact_duration(autonomy.timing.known_waiting_millis),
                    autonomy.timing.evidence_completeness,
                ),
                Style::default().fg(palette.info_fg),
            ),
        ]),
        Line::from(vec![
            status_label("Last / next:"),
            Span::styled(
                trim_line(
                    &format!("{} / {}", autonomy.last_step, autonomy.next_action),
                    72,
                ),
                Style::default().fg(palette.secondary_fg),
            ),
        ]),
        Line::from(vec![
            status_label("Wake / review:"),
            Span::styled(
                trim_line(
                    &format!(
                        "{} / {} unread ({})",
                        autonomy.wake, autonomy.unread_review_count, autonomy.review_attention
                    ),
                    72,
                ),
                Style::default().fg(palette.info_fg),
            ),
        ]),
    ]);

    if !show_guide {
        status_lines.push(Line::from(vec![
            status_label("Local browsers:"),
            Span::styled(browser_summary, Style::default().fg(palette.title_fg)),
        ]));
        status_lines.push(Line::from(vec![
            status_label("Remote dbg support:"),
            Span::styled(remote_support_summary, Style::default().fg(palette.info_fg)),
        ]));
        status_lines.push(Line::from(vec![
            status_label("Remote dbg active:"),
            Span::styled(
                remote_active_summary,
                Style::default().fg(palette.success_fg),
            ),
        ]));
        status_lines.push(Line::from(vec![
            status_label("Selected browser:"),
            Span::styled(
                selected_browser_summary,
                Style::default().fg(palette.secondary_fg),
            ),
        ]));
        status_lines.push(Line::from(vec![
            status_label("Selected target:"),
            Span::styled(
                selected_target_summary,
                Style::default().fg(palette.info_fg),
            ),
        ]));
    }

    let visible_flow_slots = if show_flow_panel {
        status_content_height.saturating_sub(status_lines.len() + 1) / flow_block_lines.max(1)
    } else {
        0
    };

    if show_flow_panel && visible_flow_slots > 0 {
        status_lines.push(Line::from(""));
        if visible_flow_count == 0 {
            let call_text = if app.remote_connected {
                "awaiting request"
            } else {
                "flow closed"
            };
            let call_offset = flow_call_offset(call_text);
            status_lines.push(Line::from(vec![
                Span::styled("    ", Style::default().fg(palette.muted_fg)),
                Span::styled(call_offset, Style::default().fg(palette.muted_fg)),
                Span::styled(call_text, flow_meta_style),
            ]));
            let lane = lane_for(false, None);
            let mut row = vec![
                Span::styled("    ", Style::default().fg(palette.muted_fg)),
                Span::styled(FLOW_LANE_LEFT_LABEL, computer_role_style),
            ];
            row.extend(lane);
            row.push(Span::styled("ChatGPT Web", chatgpt_role_style));
            row.push(Span::styled("  ", Style::default().fg(palette.muted_fg)));
            row.extend(request_stats_for(app));
            status_lines.push(Line::from(row));
        } else {
            for flow in app
                .flows
                .iter()
                .filter(|flow| should_display_flow_row(flow, app.remote_connected))
                .take(visible_flow_slots)
            {
                let latest_action = latest_flow_action(flow);
                let call_text = trim_line(&format!("call {latest_action}"), FLOW_ROW_CELLS);
                let call_offset = flow_call_offset(&call_text);
                status_lines.push(Line::from(vec![
                    Span::styled("    ", Style::default().fg(palette.muted_fg)),
                    Span::styled(call_offset, Style::default().fg(palette.muted_fg)),
                    Span::styled(call_text, flow_meta_style),
                ]));
                let closing = flow.closing_started_ms.is_some();
                let lane_active = closing
                    || !flow.anim_queue.is_empty()
                    || (app.server_running
                        && public_transport_available(app)
                        && app.remote_connected);
                let lane = lane_for(lane_active, Some(flow));
                let mut row = vec![
                    Span::styled("    ", Style::default().fg(palette.muted_fg)),
                    Span::styled(FLOW_LANE_LEFT_LABEL, computer_role_style),
                ];
                row.extend(lane);
                row.push(Span::styled("ChatGPT Web", chatgpt_role_style));
                row.push(Span::styled("  ", Style::default().fg(palette.muted_fg)));
                row.extend(request_stats_for(app));
                status_lines.push(Line::from(row));
            }
        }
    }

    if let Some(flow) = bootstrap_status_flow {
        status_lines = flow_bootstrap_status_lines(app, flow, &palette, now_millis);
    }

    let guide_step_style = Style::default()
        .fg(palette.title_fg)
        .add_modifier(Modifier::BOLD);
    let guide_text_style = Style::default().fg(palette.primary_fg);
    let guide_detail_style = Style::default().fg(palette.secondary_fg);
    let guide_strong_style = Style::default()
        .fg(palette.primary_fg)
        .add_modifier(Modifier::BOLD);
    let guide_separator_style = Style::default().fg(palette.secondary_fg);
    let guide_copyable_style = Style::default()
        .fg(palette.primary_fg)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let guide_lines = if show_guide {
        vec![
            Line::from(vec![
                Span::styled("  1. ", guide_step_style),
                Span::styled("Open connector settings: ", guide_text_style),
                Span::styled("(click to copy)", guide_detail_style),
            ]),
            Line::from(vec![
                Span::styled("     ", guide_text_style),
                Span::styled(
                    "https://chatgpt.com/apps#settings/Connectors",
                    guide_copyable_style,
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  2. ", guide_step_style),
                Span::styled("Click ", guide_text_style),
                Span::styled("Create app", guide_strong_style),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  3. ", guide_step_style),
                Span::styled("Fill in the form: ", guide_text_style),
                Span::styled("(click to copy)", guide_detail_style),
            ]),
            Line::from(vec![
                Span::styled("     Name          ", guide_detail_style),
                Span::styled(" │ ", guide_separator_style),
                Span::styled("CatDesk", guide_copyable_style),
            ]),
            Line::from(vec![
                Span::styled("     MCP Server URL", guide_detail_style),
                Span::styled(" │ ", guide_separator_style),
                Span::styled(mcp_url.clone(), guide_copyable_style),
            ]),
            Line::from(vec![
                Span::styled("     Authentication", guide_detail_style),
                Span::styled(" │ ", guide_separator_style),
                Span::styled("None", guide_copyable_style),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  4. ", guide_step_style),
                Span::styled("Click ", guide_text_style),
                Span::styled("I understand and want to continue", guide_strong_style),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  5. ", guide_step_style),
                Span::styled("Click ", guide_text_style),
                Span::styled("Create", guide_strong_style),
            ]),
        ]
    } else {
        Vec::new()
    };
    if show_guide {
        status_lines = guide_lines;
    }

    let show_mascot = area.width >= 120;
    let status_columns = if show_mascot {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(TUI_MASCOT_BLOCK_WIDTH),
            ])
            .split(chunks[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0)])
            .split(chunks[1])
    };
    let status_title = if show_guide {
        " What to do next? "
    } else if bootstrap_status_flow.is_some() {
        " MCP bootstrap "
    } else {
        " Status "
    };
    let status_block = Block::default()
        .title(status_title)
        .borders(Borders::ALL)
        .border_type(palette.border_type)
        .border_style(Style::default().fg(palette.border_fg));
    let status_inner = status_block.inner(status_columns[0]);
    f.render_widget(status_block, status_columns[0]);
    if show_mascot {
        let mascot_block = Block::default()
            .title(" Binagotchy ")
            .borders(Borders::ALL)
            .border_type(palette.border_type)
            .border_style(Style::default().fg(palette.border_fg));
        let mascot_inner = mascot_block.inner(status_columns[1]);
        f.render_widget(mascot_block, status_columns[1]);
        let mascot = Paragraph::new(render_tui_lines(
            app.mascot.current_tui_frame(now_millis),
            mascot_inner.height,
        ))
        .alignment(Alignment::Center);
        f.render_widget(mascot, mascot_inner);
    }

    let status_content = status_inner.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let status = Paragraph::new(status_lines).wrap(Wrap { trim: false });
    f.render_widget(status, status_content);

    // ── Keys ──
    let key_spans = vec![
        Span::styled("  [q]", Style::default().fg(palette.danger_fg)),
        Span::raw(" Quit  "),
        Span::styled("[Up/Down]", Style::default().fg(palette.key_fg)),
        Span::raw(" Scroll logs  "),
        Span::styled("[Wheel]", Style::default().fg(palette.key_fg)),
        Span::raw(" Scroll logs  "),
        Span::styled("[End]", Style::default().fg(palette.key_fg)),
        Span::raw(" Follow latest"),
    ];
    let keys = Paragraph::new(Line::from(key_spans)).block(
        Block::default()
            .title(" Keys ")
            .borders(Borders::ALL)
            .border_type(palette.border_type)
            .border_style(Style::default().fg(palette.border_fg)),
    );
    f.render_widget(keys, chunks[2]);

    // ── Logs ──
    let log_items: Vec<ListItem> = app
        .logs
        .iter()
        .map(|entry| {
            let color = match entry.level {
                "ERROR" => palette.danger_fg,
                "WARN" => palette.warning_fg,
                _ => palette.muted_fg,
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", entry.time),
                    Style::default().fg(palette.muted_fg),
                ),
                Span::styled(format!("{:5} ", entry.level), Style::default().fg(color)),
                Span::styled(&*entry.message, Style::default().fg(palette.primary_fg)),
            ]))
        })
        .collect();

    let visible_height = chunks[3].height.saturating_sub(2) as usize;
    let total = log_items.len();
    let max_scroll = total.saturating_sub(visible_height);
    let effective_scroll = if log_follow_tail {
        max_scroll
    } else {
        log_scroll.min(max_scroll)
    };
    *log_view = Some((max_scroll, effective_scroll));
    let visible_items: Vec<ListItem> = log_items
        .into_iter()
        .skip(effective_scroll)
        .take(visible_height)
        .collect();
    let logs = List::new(visible_items).block(
        Block::default()
            .title(" Logs ")
            .borders(Borders::ALL)
            .border_type(palette.border_type)
            .border_style(Style::default().fg(palette.border_fg)),
    );
    f.render_widget(logs, chunks[3]);

    // ── Floating toast (top-most layer) ──
    if let Some((msg, pos)) = toast {
        render_toast(f, palette, msg, pos);
    }
}
