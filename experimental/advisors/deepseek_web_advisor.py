#!/usr/bin/env python3
"""Standalone experimental DeepSeek web advisor adapter.

This adapter is intentionally outside the released CatDesk worker pipeline. It
accepts a bounded AdviceRequestV1 over authenticated JSON Lines and returns an
AdviceResponseV1-shaped object. It never receives CatDesk tool definitions and
does not expose filesystem, shell, Git, patch, job, verification, or MCP tools.
"""

from __future__ import annotations

import argparse
import contextlib
import dataclasses
import enum
import hashlib
import html.parser
import json
import os
import random
import re
import sys
import threading
import time
from pathlib import Path
from typing import Any, Iterable, TextIO
from urllib.parse import urlparse


SCHEMA_VERSION = 1
DEFAULT_MAX_REQUEST_BYTES = 24 * 1024
DEFAULT_MAX_PROMPT_BYTES = 12 * 1024
DEFAULT_MAX_RESPONSE_BYTES = 4 * 1024
REQUEST_ID_RE = re.compile(r"^[A-Za-z0-9_.:-]{1,120}$")
TRUSTED_SCHEME = "https"
TRUSTED_HOST = "chat.deepseek.com"
TRUSTED_ORIGIN = f"{TRUSTED_SCHEME}://{TRUSTED_HOST}"
REMOTE_DISCLOSURE_CLASSIFICATION = "REMOTE_ALLOWED"
DEEPSEEK_COMPOSER_SEND_SELECTOR = "deepseek:composer-send"
DEEPSEEK_COMPOSER_STOP_SELECTOR = "deepseek:composer-stop"
DEEPSEEK_CONVERSATION_BLOCKS_SELECTOR = "deepseek:conversation-visible-blocks"


class AdapterState(str, enum.Enum):
    STOPPED = "STOPPED"
    STARTING = "STARTING"
    LOGIN_REQUIRED = "LOGIN_REQUIRED"
    READY = "READY"
    SENDING = "SENDING"
    WAITING_FOR_RESPONSE = "WAITING_FOR_RESPONSE"
    COMPLETED = "COMPLETED"
    RATE_LIMITED = "RATE_LIMITED"
    TAKEOVER_REQUIRED = "TAKEOVER_REQUIRED"
    TIMED_OUT = "TIMED_OUT"
    CANCELLED = "CANCELLED"
    DEGRADED = "DEGRADED"
    FAILED = "FAILED"


class AdvisorStatus(str, enum.Enum):
    READY = "READY"
    LOGIN_REQUIRED = "LOGIN_REQUIRED"
    TAKEOVER_REQUIRED = "TAKEOVER_REQUIRED"
    RATE_LIMITED = "RATE_LIMITED"
    UNAVAILABLE = "UNAVAILABLE"
    TIMED_OUT = "TIMED_OUT"
    CANCELLED = "CANCELLED"
    COMPLETED = "COMPLETED"
    FAILED = "FAILED"
    DEGRADED = "DEGRADED"


STATE_TO_STATUS = {
    AdapterState.LOGIN_REQUIRED: AdvisorStatus.LOGIN_REQUIRED,
    AdapterState.TAKEOVER_REQUIRED: AdvisorStatus.TAKEOVER_REQUIRED,
    AdapterState.RATE_LIMITED: AdvisorStatus.RATE_LIMITED,
    AdapterState.TIMED_OUT: AdvisorStatus.TIMED_OUT,
    AdapterState.CANCELLED: AdvisorStatus.CANCELLED,
    AdapterState.COMPLETED: AdvisorStatus.COMPLETED,
    AdapterState.FAILED: AdvisorStatus.FAILED,
    AdapterState.DEGRADED: AdvisorStatus.DEGRADED,
    AdapterState.READY: AdvisorStatus.READY,
}


PROHIBITED_AUTHORITY_KEYS = {
    "tool_definitions",
    "tools",
    "filesystem",
    "shell",
    "git",
    "patch",
    "job",
    "verification",
    "mcp",
}

ALLOWED_REQUEST_KEYS = {
    "schemaversion",
    "requestid",
    "runid",
    "objective",
    "currentstep",
    "specificquestion",
    "constraints",
    "latestfailure",
    "boundedsourceexcerpts",
    "boundedpatchordiffsummary",
    "verificationsummary",
    "disclosureclassification",
    "maximumresponselength",
}


SECRET_PATTERNS = [
    re.compile(
        r"(?i)(password|passwd|pwd|token|secret|api[_-]?key|authorization)\s*[:=]\s*[\"']?[^\"'\s,;]+"
    ),
    re.compile(r"(?i)bearer\s+[A-Za-z0-9._~+/=-]{12,}"),
    re.compile(r"(?i)(gho|ghp|sk|xoxb|xoxp)_[A-Za-z0-9_=-]{12,}"),
]


DEEPSEEK_COMPOSER_SEND_STATE_SCRIPT = r"""
(() => {
  const textarea = document.querySelector('textarea[placeholder="Message DeepSeek"]');
  const result = {
    composerFound: Boolean(textarea),
    value: textarea ? String(textarea.value ?? textarea.innerText ?? '') : '',
    valueLength: textarea ? String(textarea.value ?? textarea.innerText ?? '').length : 0,
    sendVisible: false,
    sendEnabled: false,
    sendDisabled: false,
    urlPath: window.location.pathname,
    rootCount: document.querySelectorAll('.ds-virtual-list-visible-items').length,
    userTurnCount: document.querySelectorAll('.ds-virtual-list-visible-items > [data-virtual-list-item-key] .ds-message').length
  };
  if (!textarea) return result;
  function visible(element) {
    const rect = element.getBoundingClientRect();
    const style = window.getComputedStyle(element);
    return rect.width > 0 && rect.height > 0
      && style.visibility !== 'hidden' && style.display !== 'none';
  }
  function enabled(element) {
    return !element.disabled
      && element.getAttribute('aria-disabled') !== 'true'
      && !String(element.className || '').includes('ds-button--disabled');
  }
  let container = textarea;
  for (let depth = 0; container && depth < 7; depth++, container = container.parentElement) {
    const candidates = Array.from(container.querySelectorAll('button,[role="button"]'))
      .filter((element) => visible(element))
      .filter((element) => {
        const classes = String(element.className || '');
        return classes.includes('ds-button--primary') && classes.includes('ds-button--circle');
      })
      .sort((left, right) => {
        const a = left.getBoundingClientRect();
        const b = right.getBoundingClientRect();
        return (b.left - a.left) || (b.top - a.top);
      });
    if (candidates.length > 0) {
      const target = candidates[0];
      result.sendVisible = true;
      result.sendEnabled = enabled(target);
      result.sendDisabled = !result.sendEnabled;
      return result;
    }
  }
  return result;
})()
"""


DEEPSEEK_COMPOSER_SEND_CLICK_SCRIPT = r"""
(() => {
  const textarea = document.querySelector('textarea[placeholder="Message DeepSeek"]');
  const result = {
    clicked: false,
    composerFound: Boolean(textarea),
    sendVisible: false,
    sendEnabled: false,
    sendDisabled: false,
    urlPath: window.location.pathname,
    rootCount: document.querySelectorAll('.ds-virtual-list-visible-items').length,
    userTurnCount: document.querySelectorAll('.ds-virtual-list-visible-items > [data-virtual-list-item-key] .ds-message').length
  };
  if (!textarea) return result;
  function visible(element) {
    const rect = element.getBoundingClientRect();
    const style = window.getComputedStyle(element);
    return rect.width > 0 && rect.height > 0
      && style.visibility !== 'hidden' && style.display !== 'none';
  }
  function enabled(element) {
    return !element.disabled
      && element.getAttribute('aria-disabled') !== 'true'
      && !String(element.className || '').includes('ds-button--disabled');
  }
  let target = null;
  let container = textarea;
  for (let depth = 0; container && depth < 7 && !target; depth++, container = container.parentElement) {
    const candidates = Array.from(container.querySelectorAll('button,[role="button"]'))
      .filter((element) => visible(element))
      .filter((element) => {
        const classes = String(element.className || '');
        return classes.includes('ds-button--primary') && classes.includes('ds-button--circle');
      })
      .sort((left, right) => {
        const a = left.getBoundingClientRect();
        const b = right.getBoundingClientRect();
        return (b.left - a.left) || (b.top - a.top);
      });
    target = candidates[0] || null;
  }
  if (!target) return result;
  result.sendVisible = true;
  result.sendEnabled = enabled(target);
  result.sendDisabled = !result.sendEnabled;
  if (!result.sendEnabled) return result;
  target.click();
  result.clicked = true;
  result.urlPath = window.location.pathname;
  result.rootCount = document.querySelectorAll('.ds-virtual-list-visible-items').length;
  result.userTurnCount = document.querySelectorAll('.ds-virtual-list-visible-items > [data-virtual-list-item-key] .ds-message').length;
  return result;
})()
"""


DEEPSEEK_GENERATION_BOOTSTRAP_SCRIPT = r"""
(() => {
  const root = document.querySelector('.ds-virtual-list-visible-items');
  function visible(element) {
    const rect = element.getBoundingClientRect();
    const style = window.getComputedStyle(element);
    return rect.width > 0 && rect.height > 0
      && style.visibility !== 'hidden' && style.display !== 'none';
  }
  const composer = document.querySelector('textarea[placeholder="Message DeepSeek"]');
  if (!root) {
    return {
      rootFound: false,
      validEmptyChat: Boolean(composer && visible(composer)),
      turnKeys: [],
      assistantCount: 0,
      latestAssistantKey: null,
      latestAssistantText: '',
      turns: []
    };
  }
  function norm(text) {
    return String(text || '').replace(/\r\n/g, '\n')
      .replace(/[ \t]+\n/g, '\n').replace(/\n[ \t]+/g, '\n')
      .replace(/\n{3,}/g, '\n\n').trim();
  }
  function children() {
    return Array.from(root.children).filter(visible).map((turn, index) => {
      const answer = turn.querySelector('.ds-markdown.ds-assistant-message-main-content');
      const hasAssistant = Boolean(answer);
      const hasMessage = Boolean(turn.querySelector('.ds-message'));
      const text = answer ? norm(answer.innerText) : '';
      return {
        key: turn.getAttribute('data-virtual-list-item-key') || `identity:${index}`,
        index,
        role: hasAssistant ? 'assistant' : (hasMessage ? 'user' : 'other'),
        hasAssistant,
        text,
        textLength: text.length
      };
    });
  }
  const turns = children();
  const assistants = turns.filter((turn) => turn.hasAssistant);
  const latest = assistants[assistants.length - 1] || null;
  return {
    rootFound: true,
    validEmptyChat: false,
    turnKeys: turns.map((turn) => turn.key),
    assistantCount: assistants.length,
    latestAssistantKey: latest ? latest.key : null,
    latestAssistantText: latest ? latest.text : '',
    turns
  };
})()
"""


DEEPSEEK_GENERATION_INSTALL_SCRIPT = r"""
((generationId, requestId, submittedPromptHash, baselineKeys, baselineAssistantCount, baselineLatestAssistantKey, baselineLatestAssistantHash, validEmptyChat) => {
  if (window.__catdeskDeepSeekGeneration && window.__catdeskDeepSeekGeneration.disconnect) {
    window.__catdeskDeepSeekGeneration.disconnect();
  }
  const state = {
    generationId,
    requestId,
    submittedPromptHash,
    startedAt: Date.now(),
    baselineKeys,
    baselineAssistantCount,
    baselineLatestAssistantKey,
    baselineLatestAssistantHash,
    validEmptyChat,
    detectedUserTurnKey: null,
    detectedAssistantTurnKey: null,
    assistantElementKey: null,
    assistantText: '',
    previousText: '',
    lastMutationAt: Date.now(),
    lastTextChangeAt: null,
    mutationCount: 0,
    stableSampleCount: 0,
    sawUserTurn: false,
    sawAssistantTurn: false,
    sawAssistantTextChange: false,
    sawGenerationActive: false,
    assistantActionRowVisible: false,
    terminalState: null,
    lifecycle: 'WAITING_FOR_CONVERSATION_ROOT',
    events: [],
    rootObserver: null,
    bootstrapObserver: null,
    assistantObserver: null,
    rootAttached: false
  };
  function visible(element) {
    const rect = element.getBoundingClientRect();
    const style = window.getComputedStyle(element);
    return rect.width > 0 && rect.height > 0
      && style.visibility !== 'hidden' && style.display !== 'none';
  }
  function norm(text) {
    return String(text || '').replace(/\r\n/g, '\n')
      .replace(/[ \t]+\n/g, '\n').replace(/\n[ \t]+/g, '\n')
      .replace(/\n{3,}/g, '\n\n').trim();
  }
  function turnKey(turn, index) {
    return turn.getAttribute('data-virtual-list-item-key') || `identity:${index}`;
  }
  function assistantActionRowVisible(turn, answer) {
    if (!turn || !answer) return false;
    const controls = Array.from(turn.querySelectorAll('button,[role="button"]'))
      .filter(visible)
      .filter((element) => {
        return Boolean(answer.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_FOLLOWING);
      });
    return controls.length >= 2;
  }
  function classify(turn, index) {
    const answer = turn.querySelector('.ds-markdown.ds-assistant-message-main-content');
    const hasAssistant = Boolean(answer);
    const hasMessage = Boolean(turn.querySelector('.ds-message'));
    const text = answer ? norm(answer.innerText) : '';
    return {
      key: turnKey(turn, index),
      role: hasAssistant ? 'assistant' : (hasMessage ? 'user' : 'other'),
      answer,
      actionRowVisible: assistantActionRowVisible(turn, answer),
      text,
      textLength: text.length
    };
  }
  function composerReady() {
    const textarea = document.querySelector('textarea[placeholder="Message DeepSeek"], textarea.ds-scroll-area, textarea');
    if (!textarea) return {
      composerFound: false,
      composerVisible: false,
      composerEnabled: false,
      composerReadOnly: false,
      composerValueLength: 0,
      sendVisible: false,
      sendEnabled: false,
      sendDisabled: false,
      stopVisible: false
    };
    const textareaVisible = visible(textarea);
    const textareaEnabled = !textarea.disabled && textarea.getAttribute('aria-disabled') !== 'true';
    const textareaReadOnly = Boolean(textarea.readOnly) || textarea.getAttribute('readonly') !== null;
    let sendVisible = false;
    let sendEnabled = false;
    let sendDisabled = false;
    let container = textarea;
    for (let depth = 0; container && depth < 7; depth++, container = container.parentElement) {
      for (const element of Array.from(container.querySelectorAll('button,[role="button"]'))) {
        if (!visible(element)) continue;
        const classes = String(element.className || '');
        const enabled = !element.disabled
          && element.getAttribute('aria-disabled') !== 'true'
          && !classes.includes('ds-button--disabled');
        if (classes.includes('ds-button--primary') && classes.includes('ds-button--circle')) {
          sendVisible = true;
          if (enabled) sendEnabled = true;
          if (!enabled) sendDisabled = true;
        }
      }
      if (sendVisible) break;
    }
    return {
      composerFound: true,
      composerVisible: textareaVisible,
      composerEnabled: textareaEnabled,
      composerReadOnly: textareaReadOnly,
      composerValueLength: String(textarea.value ?? textarea.innerText ?? '').length,
      sendVisible,
      sendEnabled,
      sendDisabled,
      stopVisible: false
    };
  }
  function logEvent(type) {
    state.events.push({
      type,
      lifecycle: state.lifecycle,
      userKey: state.detectedUserTurnKey,
      assistantKey: state.detectedAssistantTurnKey,
      textLength: state.assistantText.length,
      mutationCount: state.mutationCount,
      timestamp: Date.now()
    });
    if (state.events.length > 80) state.events.splice(0, state.events.length - 80);
  }
  function refresh(reason) {
    const root = document.querySelector('.ds-virtual-list-visible-items');
    if (!root) {
      state.lifecycle = 'WAITING_FOR_CONVERSATION_ROOT';
      logEvent(reason);
      return { turns: [], controls: composerReady(), rootFound: false };
    }
    if (!state.rootAttached && state.attachRoot) {
      state.attachRoot(root);
    }
    const turns = Array.from(root.children).filter(visible).map(classify);
    const added = turns.filter((turn) => !baselineKeys.includes(turn.key));
    const user = added.find((turn) => turn.role === 'user');
    if (user && !state.detectedUserTurnKey) {
      state.detectedUserTurnKey = user.key;
      state.sawUserTurn = true;
    }
    const assistant = added.filter((turn) => turn.role === 'assistant').slice(-1)[0]
      || turns.filter((turn) => turn.role === 'assistant' && turn.key !== baselineLatestAssistantKey && turn.text).slice(-1)[0];
    if (assistant) {
      state.detectedAssistantTurnKey = assistant.key;
      state.sawAssistantTurn = true;
      if (assistant.answer && state.assistantElementKey !== assistant.key) {
        if (state.assistantObserver && state.assistantObserver.disconnect) {
          state.assistantObserver.disconnect();
        }
        state.assistantElementKey = assistant.key;
        state.assistantObserver = new MutationObserver((records) => {
          state.mutationCount += records.length;
          state.lastMutationAt = Date.now();
          refresh('assistant-mutation');
        });
        state.assistantObserver.observe(assistant.answer, { subtree: true, childList: true, characterData: true });
      }
      if (assistant.text !== state.assistantText) {
        state.previousText = state.assistantText;
        state.assistantText = assistant.text;
        state.lastTextChangeAt = Date.now();
        state.stableSampleCount = 0;
        if (assistant.text) state.sawAssistantTextChange = true;
      }
      state.assistantActionRowVisible = Boolean(assistant.actionRowVisible);
    }
    const controls = composerReady();
    const composerUsable = controls.composerFound && controls.composerVisible
      && controls.composerEnabled && !controls.composerReadOnly
      && (controls.composerValueLength === 0 || controls.sendEnabled);
    if (state.sawUserTurn && controls.stopVisible) state.sawGenerationActive = true;
    if (!state.sawUserTurn) state.lifecycle = 'WAITING_FOR_USER_TURN';
    else if (!state.sawAssistantTurn) state.lifecycle = 'WAITING_FOR_ASSISTANT_TURN';
    else if (!state.assistantText) state.lifecycle = 'STREAMING';
    else if (controls.stopVisible) state.lifecycle = 'STREAMING';
    else if (state.assistantActionRowVisible || composerUsable) state.lifecycle = 'STABILIZING';
    else state.lifecycle = 'STABILIZING';
    logEvent(reason);
    return { turns, controls, rootFound: true, assistantActionRowVisible: state.assistantActionRowVisible };
  }
  state.attachRoot = (root) => {
    if (!root || state.rootAttached) return false;
    if (state.bootstrapObserver) {
      state.bootstrapObserver.disconnect();
      state.bootstrapObserver = null;
    }
    state.rootObserver = new MutationObserver((records) => {
      state.mutationCount += records.length;
      state.lastMutationAt = Date.now();
      refresh('root-mutation');
    });
    state.rootObserver.observe(root, {
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
      attributeFilter: ['class', 'style', 'data-virtual-list-item-key', 'aria-disabled', 'disabled']
    });
    state.rootAttached = true;
    refresh('root-attached');
    return true;
  };
  state.disconnect = () => {
    if (state.bootstrapObserver) state.bootstrapObserver.disconnect();
    if (state.rootObserver) state.rootObserver.disconnect();
    if (state.assistantObserver) state.assistantObserver.disconnect();
    state.bootstrapObserver = null;
    state.rootObserver = null;
    state.assistantObserver = null;
  };
  window.__catdeskDeepSeekGeneration = state;
  const root = document.querySelector('.ds-virtual-list-visible-items');
  if (root) {
    state.attachRoot(root);
    return { ok: true, observer: 'root' };
  }
  if (!validEmptyChat) return { ok: false, error: 'ROOT_NOT_FOUND' };
  const appRoot = document.querySelector('#root');
  if (!appRoot) return { ok: false, error: 'APP_ROOT_NOT_FOUND' };
  state.bootstrapObserver = new MutationObserver((records) => {
    state.mutationCount += records.length;
    state.lastMutationAt = Date.now();
    const discovered = document.querySelector('.ds-virtual-list-visible-items');
    if (discovered) state.attachRoot(discovered);
    else refresh('bootstrap-mutation');
  });
  state.bootstrapObserver.observe(appRoot, { subtree: true, childList: true });
  refresh('bootstrap-installed');
  return { ok: true, observer: 'bootstrap' };
})
"""


DEEPSEEK_GENERATION_STATE_SCRIPT = r"""
(() => {
  const state = window.__catdeskDeepSeekGeneration;
  const root = document.querySelector('.ds-virtual-list-visible-items');
  if (!state) return { ok: false, error: 'NO_ACTIVE_GENERATION' };
  function visible(element) {
    const rect = element.getBoundingClientRect();
    const style = window.getComputedStyle(element);
    return rect.width > 0 && rect.height > 0
      && style.visibility !== 'hidden' && style.display !== 'none';
  }
  function norm(text) {
    return String(text || '').replace(/\r\n/g, '\n')
      .replace(/[ \t]+\n/g, '\n').replace(/\n[ \t]+/g, '\n')
      .replace(/\n{3,}/g, '\n\n').trim();
  }
  function turnKey(turn, index) {
    return turn.getAttribute('data-virtual-list-item-key') || `identity:${index}`;
  }
  function assistantActionRowVisible(turn, answer) {
    if (!turn || !answer) return false;
    const controls = Array.from(turn.querySelectorAll('button,[role="button"]'))
      .filter(visible)
      .filter((element) => {
        return Boolean(answer.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_FOLLOWING);
      });
    return controls.length >= 2;
  }
  function classify(turn, index) {
    const answer = turn.querySelector('.ds-markdown.ds-assistant-message-main-content');
    const hasAssistant = Boolean(answer);
    const hasMessage = Boolean(turn.querySelector('.ds-message'));
    const text = answer ? norm(answer.innerText) : '';
    return {
      key: turnKey(turn, index),
      role: hasAssistant ? 'assistant' : (hasMessage ? 'user' : 'other'),
      answer,
      actionRowVisible: assistantActionRowVisible(turn, answer),
      text,
      textLength: text.length
    };
  }
  function composerReady() {
    const textarea = document.querySelector('textarea[placeholder="Message DeepSeek"], textarea.ds-scroll-area, textarea');
    if (!textarea) return {
      composerFound: false,
      composerVisible: false,
      composerEnabled: false,
      composerReadOnly: false,
      composerValueLength: 0,
      sendVisible: false,
      sendEnabled: false,
      sendDisabled: false,
      stopVisible: false
    };
    const textareaVisible = visible(textarea);
    const textareaEnabled = !textarea.disabled && textarea.getAttribute('aria-disabled') !== 'true';
    const textareaReadOnly = Boolean(textarea.readOnly) || textarea.getAttribute('readonly') !== null;
    let sendVisible = false;
    let sendEnabled = false;
    let sendDisabled = false;
    let container = textarea;
    for (let depth = 0; container && depth < 7; depth++, container = container.parentElement) {
      for (const element of Array.from(container.querySelectorAll('button,[role="button"]'))) {
        if (!visible(element)) continue;
        const classes = String(element.className || '');
        const enabled = !element.disabled
          && element.getAttribute('aria-disabled') !== 'true'
          && !classes.includes('ds-button--disabled');
        if (classes.includes('ds-button--primary') && classes.includes('ds-button--circle')) {
          sendVisible = true;
          if (enabled) sendEnabled = true;
          if (!enabled) sendDisabled = true;
        }
      }
      if (sendVisible) break;
    }
    return {
      composerFound: true,
      composerVisible: textareaVisible,
      composerEnabled: textareaEnabled,
      composerReadOnly: textareaReadOnly,
      composerValueLength: String(textarea.value ?? textarea.innerText ?? '').length,
      sendVisible,
      sendEnabled,
      sendDisabled,
      stopVisible: false
    };
  }
  if (!root) {
    state.lifecycle = 'WAITING_FOR_CONVERSATION_ROOT';
    const controls = composerReady();
    return {
      ok: true,
      rootFound: false,
      lifecycle: state.lifecycle,
      generationId: state.generationId,
      requestId: state.requestId,
      detectedUserTurnKey: state.detectedUserTurnKey,
      detectedAssistantTurnKey: state.detectedAssistantTurnKey,
      assistantText: state.assistantText || '',
      assistantTextLength: String(state.assistantText || '').length,
      lastMutationAt: state.lastMutationAt,
      lastTextChangeAt: state.lastTextChangeAt,
      mutationCount: state.mutationCount,
      stableSampleCount: state.stableSampleCount,
      sawUserTurn: state.sawUserTurn,
      sawAssistantTurn: state.sawAssistantTurn,
      sawAssistantTextChange: state.sawAssistantTextChange,
      sawGenerationActive: state.sawGenerationActive,
      controls,
      turnKeys: [],
      assistantCount: 0,
      assistantActionRowVisible: Boolean(state.assistantActionRowVisible),
      urlPath: window.location.pathname,
      rootCount: document.querySelectorAll('.ds-virtual-list-visible-items').length,
      userTurnCount: document.querySelectorAll('.ds-virtual-list-visible-items > [data-virtual-list-item-key] .ds-message').length,
      bootstrapActive: Boolean(state.bootstrapObserver),
      rootObserverActive: Boolean(state.rootObserver),
      eventTail: state.events.slice(-12)
    };
  }
  if (!state.rootAttached && state.attachRoot) {
    state.attachRoot(root);
  }
  const turns = Array.from(root.children).filter(visible).map(classify);
  const added = turns.filter((turn) => !state.baselineKeys.includes(turn.key));
  const user = added.find((turn) => turn.role === 'user');
  if (user && !state.detectedUserTurnKey) {
    state.detectedUserTurnKey = user.key;
    state.sawUserTurn = true;
  }
  const assistant = added.filter((turn) => turn.role === 'assistant').slice(-1)[0]
    || turns.filter((turn) => turn.role === 'assistant' && turn.key !== state.baselineLatestAssistantKey && turn.text).slice(-1)[0];
  if (assistant) {
    state.detectedAssistantTurnKey = assistant.key;
    state.sawAssistantTurn = true;
    if (assistant.answer && (!state.assistantElementKey || state.assistantElementKey !== assistant.key)) {
      if (state.assistantObserver && state.assistantObserver.disconnect) state.assistantObserver.disconnect();
      state.assistantObserver = new MutationObserver((records) => {
        state.mutationCount += records.length;
        state.lastMutationAt = Date.now();
      });
      state.assistantObserver.observe(assistant.answer, { subtree: true, childList: true, characterData: true });
      state.assistantElementKey = assistant.key;
    }
    if (assistant.text !== state.assistantText) {
      state.previousText = state.assistantText;
      state.assistantText = assistant.text;
      state.lastTextChangeAt = Date.now();
      state.stableSampleCount = 0;
      if (assistant.text) state.sawAssistantTextChange = true;
    }
    state.assistantActionRowVisible = Boolean(assistant.actionRowVisible);
  }
  const controls = composerReady();
  const composerUsable = controls.composerFound && controls.composerVisible
    && controls.composerEnabled && !controls.composerReadOnly
    && (controls.composerValueLength === 0 || controls.sendEnabled);
  if (state.sawUserTurn && controls.stopVisible) state.sawGenerationActive = true;
  if (!state.sawUserTurn) state.lifecycle = 'WAITING_FOR_USER_TURN';
  else if (!state.sawAssistantTurn) state.lifecycle = 'WAITING_FOR_ASSISTANT_TURN';
  else if (!state.assistantText) state.lifecycle = 'STREAMING';
  else if (controls.stopVisible) state.lifecycle = 'STREAMING';
  else if (state.assistantActionRowVisible || composerUsable) state.lifecycle = 'STABILIZING';
  else state.lifecycle = 'STABILIZING';
  return {
    ok: true,
    rootFound: true,
    lifecycle: state.lifecycle,
    generationId: state.generationId,
    requestId: state.requestId,
    detectedUserTurnKey: state.detectedUserTurnKey,
    detectedAssistantTurnKey: state.detectedAssistantTurnKey,
    assistantText: state.assistantText,
    assistantTextLength: state.assistantText.length,
    lastMutationAt: state.lastMutationAt,
    lastTextChangeAt: state.lastTextChangeAt,
    mutationCount: state.mutationCount,
    stableSampleCount: state.stableSampleCount,
    sawUserTurn: state.sawUserTurn,
    sawAssistantTurn: state.sawAssistantTurn,
    sawAssistantTextChange: state.sawAssistantTextChange,
    sawGenerationActive: state.sawGenerationActive,
    controls,
    turnKeys: turns.map((turn) => turn.key),
    assistantCount: turns.filter((turn) => turn.role === 'assistant').length,
    assistantActionRowVisible: Boolean(state.assistantActionRowVisible),
    urlPath: window.location.pathname,
    rootCount: document.querySelectorAll('.ds-virtual-list-visible-items').length,
    userTurnCount: document.querySelectorAll('.ds-virtual-list-visible-items > [data-virtual-list-item-key] .ds-message').length,
    bootstrapActive: Boolean(state.bootstrapObserver),
    rootObserverActive: Boolean(state.rootObserver),
    eventTail: state.events.slice(-12)
  };
})()
"""


DEEPSEEK_GENERATION_DISCONNECT_SCRIPT = r"""
(() => {
  const state = window.__catdeskDeepSeekGeneration;
  if (!state) return false;
  if (state.disconnect) state.disconnect();
  delete window.__catdeskDeepSeekGeneration;
  return true;
})()
"""


DEEPSEEK_VISIBLE_CONVERSATION_BLOCKS_SCRIPT = r"""
(() => {
  const requestedRole = "__CATDESK_ROLE__";
  const textarea = document.querySelector('textarea');
  if (!textarea) return [];
  const textareaRect = textarea.getBoundingClientRect();
  const all = Array.from(document.querySelectorAll('main article,main section,main div,main p,main pre'));
  const candidates = all
    .filter((element) => {
      if (element.contains(textarea)) return false;
      const rect = element.getBoundingClientRect();
      const style = window.getComputedStyle(element);
      if (rect.width <= 0 || rect.height <= 0) return false;
      if (style.visibility === 'hidden' || style.display === 'none') return false;
      if (rect.bottom >= textareaRect.top - 8) return false;
      const horizontalOverlap = Math.min(rect.right, textareaRect.right) - Math.max(rect.left, textareaRect.left);
      if (horizontalOverlap <= Math.min(rect.width, textareaRect.width) * 0.20) return false;
      const text = (element.innerText || '').replace(/\s+/g, ' ').trim();
      if (text.length < 12 || text.length > 6000) return false;
      const userish = rect.left > window.innerWidth * 0.42 && rect.width < textareaRect.width * 0.92;
      if (requestedRole === "user") return userish;
      return !userish;
    })
    .filter((element, _index, list) => {
      const text = (element.innerText || '').replace(/\s+/g, ' ').trim();
      return !list.some((other) => {
        if (other === element) return false;
        if (!element.contains(other)) return false;
        const otherText = (other.innerText || '').replace(/\s+/g, ' ').trim();
        return otherText.length >= 12 && text.includes(otherText);
      });
    })
    .sort((left, right) => {
      const a = left.getBoundingClientRect();
      const b = right.getBoundingClientRect();
      return (a.top - b.top) || (a.left - b.left);
    });
  const output = [];
  for (const element of candidates) {
    const text = (element.innerText || '').replace(/\s+/g, ' ').trim();
    if (!text) continue;
    if (output.some((existing) => existing === text || existing.includes(text))) continue;
    output.push(text);
  }
  return output.slice(-20);
})()
"""


@dataclasses.dataclass(frozen=True)
class SelectorConfig:
    start_url: str
    response_container_selectors: list[str]
    assistant_message_selectors: list[str]
    user_message_selectors: list[str]
    prompt_input_selectors: list[str]
    send_button_selectors: list[str]
    stop_button_selectors: list[str]
    login_required_selectors: list[str]
    login_email_selectors: list[str]
    login_password_selectors: list[str]
    login_submit_selectors: list[str]
    takeover_required_selectors: list[str]
    cookie_banner_selectors: list[str]
    cookie_reject_selectors: list[str]
    cookie_accept_selectors: list[str]
    rate_limit_selectors: list[str]
    chat_menu_button_selectors: list[str]
    chat_item_selectors: list[str]
    current_chat_title_selectors: list[str]
    text_stability_seconds: float = 5.0
    stable_sample_count: int = 3
    timeout_seconds: float = 120.0
    submission_confirmation_timeout_seconds: float = 8.0
    typing_min_interval_seconds: float = 0.03
    typing_max_interval_seconds: float = 0.09
    typing_newline_pause_min_seconds: float = 0.15
    typing_newline_pause_max_seconds: float = 0.35
    typing_timeout_seconds: float = 240.0
    bounded_dom_evidence_bytes: int = 2048

    @classmethod
    def from_file(cls, path: Path) -> "SelectorConfig":
        with path.open("r", encoding="utf-8") as handle:
            value = json.load(handle)
        value.setdefault("assistant_message_selectors", value.get("response_container_selectors", []))
        value.setdefault("user_message_selectors", [])
        value.setdefault("login_email_selectors", [])
        value.setdefault("login_password_selectors", [])
        value.setdefault("login_submit_selectors", [])
        value.setdefault("cookie_banner_selectors", [])
        value.setdefault("cookie_reject_selectors", [])
        value.setdefault("cookie_accept_selectors", [])
        value.setdefault("rate_limit_selectors", [])
        value.setdefault("chat_menu_button_selectors", [])
        value.setdefault("chat_item_selectors", [])
        value.setdefault("current_chat_title_selectors", [])
        value.setdefault("stable_sample_count", 3)
        value.setdefault("submission_confirmation_timeout_seconds", 8.0)
        value.setdefault("typing_min_interval_seconds", 0.03)
        value.setdefault("typing_max_interval_seconds", 0.09)
        if "typing_newline_pause_seconds" in value:
            value.setdefault("typing_newline_pause_min_seconds", value["typing_newline_pause_seconds"])
            value.setdefault("typing_newline_pause_max_seconds", value["typing_newline_pause_seconds"])
            value.pop("typing_newline_pause_seconds", None)
        value.setdefault("typing_newline_pause_min_seconds", 0.15)
        value.setdefault("typing_newline_pause_max_seconds", 0.35)
        value.setdefault("typing_timeout_seconds", 240.0)
        value.pop("rate_limit_markers", None)
        value.pop("completion_markers", None)
        return cls(**value)


@dataclasses.dataclass(frozen=True)
class AdviceResponseV1:
    schema_version: int
    request_id: str
    advisor_id: str
    status: str
    diagnosis: str
    recommendations: list[str]
    risks: list[str]
    assumptions_or_questions: list[str]
    confidence: str
    raw_artifact_reference: str | None

    def to_dict(self) -> dict[str, Any]:
        return dataclasses.asdict(self)


@dataclasses.dataclass(frozen=True)
class RedactedDiagnostics:
    state: str
    failed_selector: str | None
    page_title: str
    url_origin: str
    bounded_dom_evidence: str

    def to_dict(self) -> dict[str, str | None]:
        return dataclasses.asdict(self)


@dataclasses.dataclass
class VirtualTurn:
    key: str
    role: str
    assistant_text: str
    assistant_hash: str
    text_length: int


@dataclasses.dataclass
class VirtualListBaseline:
    root_found: bool
    turn_keys: list[str]
    assistant_count: int
    latest_assistant_key: str | None
    latest_assistant_hash: str | None
    turns: list[VirtualTurn]
    valid_empty_chat: bool = False


@dataclasses.dataclass
class GenerationTracker:
    generation_id: str
    request_id: str
    submitted_prompt_hash: str
    started_at: float
    baseline_turn_keys: set[str]
    baseline_assistant_count: int
    baseline_latest_assistant_key: str | None
    baseline_latest_assistant_hash: str | None
    detected_user_turn_key: str | None = None
    detected_assistant_turn_key: str | None = None
    assistant_element_identity: str | None = None
    assistant_text: str = ""
    assistant_text_hash: str = ""
    previous_text_hash: str = ""
    last_mutation_at: float | None = None
    last_text_change_at: float | None = None
    mutation_count: int = 0
    stable_sample_count: int = 0
    saw_user_turn: bool = False
    saw_assistant_turn: bool = False
    saw_assistant_text_change: bool = False
    saw_generation_active: bool = False
    generation_active_now: bool = False
    generation_inactive_sample_count: int = 0
    stable_duration_seconds: float = 0.0
    composer_found: bool = False
    composer_visible: bool = False
    composer_enabled: bool = False
    composer_read_only: bool = False
    composer_value_length: int = 0
    send_visible: bool = False
    send_enabled: bool = False
    stop_visible: bool = False
    assistant_action_row_visible: bool = False
    completion_blockers: dict[str, bool] = dataclasses.field(default_factory=dict)
    terminal_state: str | None = None
    cancellation_event: threading.Event = dataclasses.field(default_factory=threading.Event)

    def redacted_diagnostics(self) -> dict[str, Any]:
        return {
            "generation_id": self.generation_id,
            "request_id": self.request_id,
            "baseline_turn_count": len(self.baseline_turn_keys),
            "baseline_assistant_count": self.baseline_assistant_count,
            "detected_user_turn_key": self.detected_user_turn_key,
            "detected_assistant_turn_key": self.detected_assistant_turn_key,
            "assistant_text_length": len(self.assistant_text),
            "assistant_text_hash": self.assistant_text_hash,
            "mutation_count": self.mutation_count,
            "stable_sample_count": self.stable_sample_count,
            "saw_user_turn": self.saw_user_turn,
            "saw_assistant_turn": self.saw_assistant_turn,
            "saw_assistant_text_change": self.saw_assistant_text_change,
            "saw_generation_active": self.saw_generation_active,
            "generation_active_now": self.generation_active_now,
            "generation_inactive_sample_count": self.generation_inactive_sample_count,
            "stable_duration_seconds": round(self.stable_duration_seconds, 3),
            "composer_found": self.composer_found,
            "composer_visible": self.composer_visible,
            "composer_enabled": self.composer_enabled,
            "composer_read_only": self.composer_read_only,
            "composer_value_length": self.composer_value_length,
            "send_visible": self.send_visible,
            "send_enabled": self.send_enabled,
            "stop_visible": self.stop_visible,
            "assistant_action_row_visible": self.assistant_action_row_visible,
            "completion_blockers": self.completion_blockers,
            "terminal_state": self.terminal_state,
        }


@dataclasses.dataclass
class SimpleDomNode:
    tag: str
    attrs: dict[str, str]
    children: list["SimpleDomNode"] = dataclasses.field(default_factory=list)
    text_parts: list[str] = dataclasses.field(default_factory=list)
    parent: "SimpleDomNode | None" = None

    def has_class(self, name: str) -> bool:
        return name in self.attrs.get("class", "").split()

    def visible(self) -> bool:
        return attrs_are_visible(self.attrs)

    def text(self, *, preserve_lines: bool = False) -> str:
        parts: list[str] = []
        for part in self.text_parts:
            if part.strip():
                parts.append(part)
        for child in self.children:
            child_text = child.text(preserve_lines=preserve_lines)
            if child_text:
                parts.append(child_text)
        joined = "\n".join(parts) if preserve_lines else " ".join(parts)
        return normalize_response_text(joined) if preserve_lines else normalize_plain_text(joined)

    def descendants(self) -> list["SimpleDomNode"]:
        output: list[SimpleDomNode] = []
        for child in self.children:
            output.append(child)
            output.extend(child.descendants())
        return output

    def first_descendant_with_classes(self, classes: set[str]) -> "SimpleDomNode | None":
        for node in self.descendants():
            node_classes = set(node.attrs.get("class", "").split())
            if classes.issubset(node_classes):
                return node
        return None

    def has_descendant_with_class(self, class_name: str) -> bool:
        return any(node.has_class(class_name) for node in self.descendants())


class SimpleDomParser(html.parser.HTMLParser):
    VOID_TAGS = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"}

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.root = SimpleDomNode("document", {})
        self.stack = [self.root]

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        node = SimpleDomNode(
            tag.lower(),
            {key.lower(): value or "" for key, value in attrs},
            parent=self.stack[-1],
        )
        self.stack[-1].children.append(node)
        if tag.lower() not in self.VOID_TAGS:
            self.stack.append(node)

    def handle_endtag(self, tag: str) -> None:
        tag = tag.lower()
        for index in range(len(self.stack) - 1, 0, -1):
            if self.stack[index].tag == tag:
                del self.stack[index:]
                return

    def handle_data(self, data: str) -> None:
        if data.strip():
            self.stack[-1].text_parts.append(data)


class MinimalHtml(html.parser.HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.nodes: list[tuple[str, dict[str, str]]] = []
        self.text_parts: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        self.nodes.append((tag.lower(), {k.lower(): v or "" for k, v in attrs}))

    def handle_data(self, data: str) -> None:
        if data.strip():
            self.text_parts.append(data.strip())

    @property
    def text(self) -> str:
        return " ".join(self.text_parts)


class ResponseTextHtml(html.parser.HTMLParser):
    def __init__(self, selectors: Iterable[str]) -> None:
        super().__init__(convert_charrefs=True)
        self.selectors = list(selectors)
        self.responses: list[str] = []
        self._depth = 0
        self._parts: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        normalized_attrs = {k.lower(): v or "" for k, v in attrs}
        if self._depth > 0:
            self._depth += 1
            return
        if (
            any(selector_matches(selector, tag.lower(), normalized_attrs) for selector in self.selectors)
            and attrs_are_visible(normalized_attrs)
        ):
            self._depth = 1
            self._parts = []

    def handle_endtag(self, _tag: str) -> None:
        if self._depth == 0:
            return
        self._depth -= 1
        if self._depth == 0:
            self.responses.append(normalize_plain_text(" ".join(self._parts)))
            self._parts = []

    def handle_data(self, data: str) -> None:
        if self._depth > 0 and data.strip():
            self._parts.append(data.strip())


@dataclasses.dataclass(frozen=True)
class PageSnapshot:
    html: str
    title: str
    url: str

    @property
    def origin(self) -> str:
        parsed = urlparse(self.url)
        if not parsed.scheme or not parsed.netloc:
            return "unknown"
        return f"{parsed.scheme}://{parsed.netloc}"

    @property
    def parsed(self) -> MinimalHtml:
        parser = MinimalHtml()
        parser.feed(self.html)
        return parser

    def selector_count(self, selector: str) -> int:
        return sum(1 for tag, attrs in self.parsed.nodes if selector_matches(selector, tag, attrs))

    def visible_selector_count(self, selector: str) -> int:
        return sum(
            1
            for tag, attrs in self.parsed.nodes
            if selector_matches(selector, tag, attrs) and attrs_are_visible(attrs)
        )

    def selector_is_visible(self, selector: str) -> bool:
        return self.visible_selector_count(selector) > 0

    def selector_is_enabled(self, selector: str) -> bool:
        for tag, attrs in self.parsed.nodes:
            if selector_matches(selector, tag, attrs) and attrs_are_visible(attrs):
                if attrs.get("disabled") is not None:
                    return False
                if attrs.get("aria-disabled", "").lower() == "true":
                    return False
                return True
        return False

    def any_selector(self, selectors: Iterable[str]) -> tuple[bool, str | None]:
        for selector in selectors:
            if self.selector_is_visible(selector):
                return True, selector
        return False, None

    def redacted_dom_evidence(self, max_bytes: int) -> str:
        evidence_parts = []
        for tag, attrs in self.parsed.nodes:
            safe_attrs = []
            for key in sorted(attrs):
                if key in {"aria-label", "data-testid", "data-ds-role", "data-role", "role", "type"}:
                    safe_attrs.append(f'{key}="{redact(attrs[key])}"')
                elif key in {"id", "class", "src", "href"}:
                    safe_attrs.append(f'{key}="<redacted>"')
            joined = " ".join(safe_attrs)
            evidence_parts.append(f"<{tag}{(' ' + joined) if joined else ''}>")
        return bound_text(" ".join(evidence_parts), max_bytes)

    def response_texts(self, selectors: Iterable[str]) -> list[str]:
        parser = ResponseTextHtml(selectors)
        parser.feed(self.html)
        return parser.responses


@dataclasses.dataclass(frozen=True)
class CompletionObservation:
    state: AdapterState
    response_count: int
    stop_visible: bool
    send_visible: bool
    stable_for_seconds: float
    marker_seen: bool
    diagnostics: RedactedDiagnostics


def selector_matches(selector: str, tag: str, attrs: dict[str, str]) -> bool:
    selector = selector.strip()
    if not selector:
        return False
    if selector.startswith("deepseek:"):
        return False
    tag_match = re.match(r"^([a-zA-Z][a-zA-Z0-9_-]*)", selector)
    wanted_tag = tag_match.group(1).lower() if tag_match else None
    if wanted_tag and wanted_tag != tag:
        return False
    if selector.startswith("."):
        return selector[1:] in attrs.get("class", "").split()
    if selector.startswith("#"):
        return attrs.get("id") == selector[1:]
    attr_match = re.search(r"\[([a-zA-Z0-9_-]+)([*^$|~]?=)?\"?([^\]\"]*)\"?\]", selector)
    if attr_match:
        name, op, expected = attr_match.groups()
        actual = attrs.get(name.lower())
        if actual is None:
            return False
        if not op:
            return True
        if op == "*=":
            return expected in actual
        return actual == expected
    return wanted_tag == tag


def attrs_are_visible(attrs: dict[str, str]) -> bool:
    if "hidden" in attrs:
        return False
    if attrs.get("aria-hidden", "").lower() == "true":
        return False
    style = attrs.get("style", "").lower().replace(" ", "")
    return "display:none" not in style and "visibility:hidden" not in style


class CompletionDetector:
    def __init__(self, selectors: SelectorConfig) -> None:
        self.selectors = selectors

    def observe(
        self,
        snapshot: PageSnapshot,
        *,
        first_seen_at: float,
        last_text_change_at: float,
        now: float,
    ) -> CompletionObservation:
        takeover, takeover_selector = snapshot.any_selector(
            self.selectors.takeover_required_selectors
        )
        login_required, login_selector = snapshot.any_selector(
            self.selectors.login_required_selectors
        )
        rate_limited, rate_limit_selector = snapshot.any_selector(self.selectors.rate_limit_selectors)
        response_texts = snapshot.response_texts(self.selectors.response_container_selectors)
        newest_text = response_texts[-1] if response_texts else ""
        response_count = len(response_texts)
        stop_visible, stop_selector = snapshot.any_selector(self.selectors.stop_button_selectors)
        send_visible, send_selector = snapshot.any_selector(self.selectors.send_button_selectors)
        marker_seen = bool(newest_text)
        stable_for = max(0.0, now - last_text_change_at)
        elapsed = max(0.0, now - first_seen_at)
        failed_selector = None

        if takeover:
            state = AdapterState.TAKEOVER_REQUIRED
            failed_selector = takeover_selector
        elif login_required:
            state = AdapterState.LOGIN_REQUIRED
            failed_selector = login_selector
        elif rate_limited:
            state = AdapterState.RATE_LIMITED
            failed_selector = rate_limit_selector
        elif elapsed >= self.selectors.timeout_seconds:
            state = AdapterState.TIMED_OUT
        elif (
            response_count > 0
            and not stop_visible
            and send_visible
            and stable_for >= self.selectors.text_stability_seconds
        ):
            state = AdapterState.COMPLETED
        else:
            state = AdapterState.WAITING_FOR_RESPONSE
            if response_count == 0:
                failed_selector = ",".join(self.selectors.response_container_selectors)
            elif stop_visible:
                failed_selector = stop_selector
            elif not send_visible:
                failed_selector = ",".join(self.selectors.send_button_selectors)
            elif not newest_text:
                failed_selector = "newest_assistant_response"

        diagnostics = RedactedDiagnostics(
            state=state.value,
            failed_selector=failed_selector or send_selector,
            page_title=redact(snapshot.title),
            url_origin=snapshot.origin,
            bounded_dom_evidence=snapshot.redacted_dom_evidence(
                self.selectors.bounded_dom_evidence_bytes
            ),
        )
        return CompletionObservation(
            state=state,
            response_count=response_count,
            stop_visible=stop_visible,
            send_visible=send_visible,
            stable_for_seconds=stable_for,
            marker_seen=marker_seen,
            diagnostics=diagnostics,
        )


@dataclasses.dataclass(frozen=True)
class ValidatedAdviceRequest:
    raw: dict[str, Any]
    request_id: str
    objective: str
    current_step: str
    specific_question: str
    constraints: list[str]
    latest_failure: str | None
    bounded_source_excerpts: list[dict[str, str]]
    bounded_patch_or_diff_summary: str | None
    verification_summary: str | None
    maximum_response_length: int


def normalize_key(value: str) -> str:
    return re.sub(r"[^a-z0-9]", "", value.lower())


def get_field(request: dict[str, Any], normalized_name: str, default: Any = None) -> Any:
    for key, value in request.items():
        if normalize_key(str(key)) == normalized_name:
            return value
    return default


def validate_advice_request(request: Any) -> ValidatedAdviceRequest:
    if not isinstance(request, dict):
        raise ValueError("REQUEST_MUST_BE_OBJECT")
    serialized = json.dumps(request, separators=(",", ":"), sort_keys=True)
    if len(serialized.encode("utf-8")) > DEFAULT_MAX_REQUEST_BYTES:
        raise ValueError("REQUEST_TOO_LARGE")
    authority_error = contains_prohibited_authority(request)
    if authority_error:
        raise ValueError(f"PROHIBITED_AUTHORITY:{authority_error}")
    unexpected = sorted(
        str(key)
        for key in request
        if normalize_key(str(key)) not in ALLOWED_REQUEST_KEYS
    )
    if unexpected:
        raise ValueError(f"UNSUPPORTED_REQUEST_KEYS:{','.join(unexpected)}")
    if get_field(request, "schemaversion") != SCHEMA_VERSION:
        raise ValueError("UNSUPPORTED_SCHEMA_VERSION")
    disclosure_classification = str(
        get_field(request, "disclosureclassification", "")
    ).strip()
    if disclosure_classification != REMOTE_DISCLOSURE_CLASSIFICATION:
        raise ValueError("DISCLOSURE_DENIED")

    request_id = str(get_field(request, "requestid", "")).strip()
    if not REQUEST_ID_RE.match(request_id):
        raise ValueError("INVALID_REQUEST_ID")
    objective = str(get_field(request, "objective", "")).strip()
    specific_question = str(get_field(request, "specificquestion", "")).strip()
    if not objective:
        raise ValueError("MISSING_OBJECTIVE")
    if not specific_question:
        raise ValueError("MISSING_SPECIFIC_QUESTION")

    maximum_response_length = get_field(
        request,
        "maximumresponselength",
        DEFAULT_MAX_RESPONSE_BYTES,
    )
    try:
        maximum_response_length = int(maximum_response_length)
    except (TypeError, ValueError):
        raise ValueError("INVALID_MAXIMUM_RESPONSE_LENGTH") from None
    maximum_response_length = max(1, min(maximum_response_length, DEFAULT_MAX_RESPONSE_BYTES))

    constraints = get_field(request, "constraints", [])
    if constraints is None:
        constraints = []
    if not isinstance(constraints, list):
        raise ValueError("INVALID_CONSTRAINTS")
    excerpts = get_field(request, "boundedsourceexcerpts", [])
    if excerpts is None:
        excerpts = []
    if not isinstance(excerpts, list):
        raise ValueError("INVALID_EXCERPTS")

    normalized_excerpts = []
    for excerpt in excerpts:
        if not isinstance(excerpt, dict):
            raise ValueError("INVALID_EXCERPT")
        normalized_excerpts.append(
            {
                "source": bound_text(redact(str(get_field(excerpt, "source", ""))), 512),
                "summary": bound_text(redact(str(get_field(excerpt, "summary", ""))), 1024),
                "content": bound_text(redact(str(get_field(excerpt, "content", ""))), 2048),
            }
        )

    return ValidatedAdviceRequest(
        raw=request,
        request_id=request_id,
        objective=bound_text(redact(objective), 2048),
        current_step=bound_text(redact(str(get_field(request, "currentstep", ""))), 1024),
        specific_question=bound_text(redact(specific_question), 2048),
        constraints=[bound_text(redact(str(item)), 512) for item in constraints],
        latest_failure=optional_bounded(request, "latestfailure"),
        bounded_source_excerpts=normalized_excerpts,
        bounded_patch_or_diff_summary=optional_bounded(
            request,
            "boundedpatchordiffsummary",
        ),
        verification_summary=optional_bounded(request, "verificationsummary"),
        maximum_response_length=maximum_response_length,
    )


def optional_bounded(request: dict[str, Any], normalized_name: str) -> str | None:
    value = get_field(request, normalized_name)
    if value is None:
        return None
    text = str(value).strip()
    if not text:
        return None
    return bound_text(redact(text), 2048)


def build_advisory_prompt(request: ValidatedAdviceRequest) -> str:
    sections = [
        "You are an external advisory model. You have no CatDesk tools and no execution authority.",
        "Return concise debugging advice only. Do not claim to inspect files that are not included.",
        f"Request ID: {request.request_id}",
        f"Objective:\n{request.objective}",
        f"Current step:\n{request.current_step}",
        f"Specific question:\n{request.specific_question}",
    ]
    if request.constraints:
        sections.append("Constraints:\n" + "\n".join(f"- {item}" for item in request.constraints))
    if request.latest_failure:
        sections.append(f"Latest failure:\n{request.latest_failure}")
    if request.bounded_patch_or_diff_summary:
        sections.append(f"Patch or diff summary:\n{request.bounded_patch_or_diff_summary}")
    if request.verification_summary:
        sections.append(f"Verification summary:\n{request.verification_summary}")
    for index, excerpt in enumerate(request.bounded_source_excerpts, start=1):
        sections.append(
            "\n".join(
                [
                    f"Bounded source excerpt {index}:",
                    f"Source: {excerpt['source']}",
                    f"Summary: {excerpt['summary']}",
                    excerpt["content"],
                ]
            )
        )
    return bound_text("\n\n".join(sections), DEFAULT_MAX_PROMPT_BYTES)


def normalize_advice_response(
    request: ValidatedAdviceRequest,
    advisor_id: str,
    status: AdvisorStatus,
    text: str,
    *,
    confidence: str = "MEDIUM",
) -> AdviceResponseV1:
    return AdviceResponseV1(
        schema_version=SCHEMA_VERSION,
        request_id=request.request_id,
        advisor_id=advisor_id,
        status=status.value,
        diagnosis=bound_text(normalize_plain_text(redact(text)), request.maximum_response_length),
        recommendations=[],
        risks=[],
        assumptions_or_questions=[],
        confidence=confidence,
        raw_artifact_reference=None,
    )


class DeepSeekWebAdvisorAdapter:
    """Experimental headed browser adapter using SeleniumBase CDP mode.

    Browser launch is opt-in through start(). The constructor and offline tests
    do not import or start SeleniumBase.
    """

    advisor_id = "deepseek-web-advisor-experimental"

    def __init__(
        self,
        selectors: SelectorConfig,
        profile_dir: Path,
        *,
        headed: bool = True,
        allow_env_login: bool = False,
        chat_name: str | None = None,
        rng: random.Random | None = None,
    ) -> None:
        self.selectors = selectors
        self.profile_dir = profile_dir
        self.headed = headed
        self.allow_env_login = allow_env_login
        self.chat_name = chat_name.strip() if chat_name else None
        self.rng = rng or random.Random()
        self.state = AdapterState.STOPPED
        self._sb_context: Any = None
        self._sb: Any = None
        self.detector = CompletionDetector(selectors)
        self.cancel_requested = threading.Event()
        self.cookie_banner_result = "unknown"
        self.last_submission_diagnostics: dict[str, Any] = {}
        self.active_generation: GenerationTracker | None = None

    def start(self) -> AdapterState:
        self.profile_dir.mkdir(parents=True, exist_ok=True)
        self.state = AdapterState.STARTING
        self.cookie_banner_result = "unknown"
        try:
            if not self.selector_start_url_is_trusted():
                self.state = AdapterState.DEGRADED
                return self.state
            with contextlib.redirect_stdout(sys.stderr):
                SB = self._import_seleniumbase()
                self._sb_context = SB(
                    uc=True,
                    test=True,
                    headless=not self.headed,
                    user_data_dir=str(self.profile_dir),
                )
                self._sb = self._sb_context.__enter__()
                self._sb.activate_cdp_mode(self.selectors.start_url)
            self._handle_cookie_banner()
            return self.refresh_state()
        except Exception as exc:
            sys.stderr.write(f"DeepSeek advisor startup failed: {type(exc).__name__}\n")
            self._cleanup_browser_context()
            self.state = AdapterState.DEGRADED
            return self.state

    def _import_seleniumbase(self) -> Any:
        from seleniumbase import SB  # type: ignore[import-not-found]

        return SB

    def stop(self) -> None:
        self.state = AdapterState.STOPPED
        self._disconnect_generation_observers()
        self.active_generation = None
        self._cleanup_browser_context()
        self.cancel_requested.clear()

    def _cleanup_browser_context(self) -> None:
        if self._sb_context is not None:
            with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                self._sb_context.__exit__(None, None, None)
        self._sb_context = None
        self._sb = None

    def refresh_state(self) -> AdapterState:
        if self._sb is None:
            self.state = AdapterState.STOPPED
            return self.state
        try:
            snapshot = self._snapshot()
        except Exception:
            self.state = AdapterState.DEGRADED
            return self.state
        if snapshot.origin != self.expected_origin:
            self.state = AdapterState.DEGRADED
            return self.state
        if self._handle_cookie_banner(snapshot):
            snapshot = self._snapshot()
        takeover, _selector = snapshot.any_selector(self.selectors.takeover_required_selectors)
        if takeover:
            self.state = AdapterState.TAKEOVER_REQUIRED
            return self.state
        login_required, _selector = snapshot.any_selector(self.selectors.login_required_selectors)
        if login_required:
            if self.allow_env_login and self._attempt_env_login():
                snapshot = self._snapshot()
                login_required, _selector = snapshot.any_selector(
                    self.selectors.login_required_selectors
                )
                takeover, _selector = snapshot.any_selector(
                    self.selectors.takeover_required_selectors
                )
                if takeover:
                    self.state = AdapterState.TAKEOVER_REQUIRED
                    return self.state
                if not login_required:
                    return self.refresh_state()
            self.state = AdapterState.LOGIN_REQUIRED
            return self.state
        rate_limited, _selector = snapshot.any_selector(self.selectors.rate_limit_selectors)
        if rate_limited:
            self.state = AdapterState.RATE_LIMITED
            return self.state
        stop_visible = self._any_visible(snapshot, self.selectors.stop_button_selectors)
        prompt_ready, _selector = self._any_visible_selector(
            snapshot,
            self.selectors.prompt_input_selectors,
        )
        if prompt_ready and not stop_visible:
            if self.chat_name and not self._ensure_chat_selected(self.chat_name):
                self.state = AdapterState.DEGRADED
                return self.state
            self.state = AdapterState.READY
            return self.state
        self.state = AdapterState.DEGRADED
        return self.state

    @property
    def expected_origin(self) -> str:
        return TRUSTED_ORIGIN

    def selector_start_url_is_trusted(self) -> bool:
        parsed = urlparse(self.selectors.start_url)
        return (
            parsed.scheme == TRUSTED_SCHEME
            and parsed.hostname == TRUSTED_HOST
            and parsed.username is None
            and parsed.password is None
        )

    def advise(self, request: dict[str, Any]) -> AdviceResponseV1:
        validated = validate_advice_request(request)
        try:
            if self.refresh_state() != AdapterState.READY:
                return self.advice_unavailable(validated.request_id)
            snapshot = self._snapshot()
            if snapshot.origin != self.expected_origin:
                self.state = AdapterState.DEGRADED
                return self.advice_unavailable(validated.request_id)

            prompt = build_advisory_prompt(validated)
            prompt_selector = self._first_enabled_visible_selector(
                snapshot,
                self.selectors.prompt_input_selectors,
            )
            if self._live_browser_available():
                composer_state = self._deepseek_composer_send_state()
                if not composer_state.get("composerFound"):
                    self.state = AdapterState.DEGRADED
                    return self.advice_degraded(
                        validated.request_id,
                        "DeepSeek exact composer textarea was not available for live submission.",
                    )
                prompt_selector = 'textarea[placeholder="Message DeepSeek"]'
            if prompt_selector is None:
                self.state = AdapterState.DEGRADED
                return self.advice_unavailable(validated.request_id)

            self.cancel_requested.clear()
            browser_baseline = self._browser_virtual_list_baseline()
            use_generation_observer = bool(
                browser_baseline
                and (browser_baseline.root_found or browser_baseline.valid_empty_chat)
            )
            tracker: GenerationTracker | None = None
            if use_generation_observer and browser_baseline is not None:
                tracker = GenerationTracker(
                    generation_id=f"gen-{validated.request_id}-{int(self._now() * 1000)}",
                    request_id=validated.request_id,
                    submitted_prompt_hash=sha256_text(prompt),
                    started_at=self._now(),
                    baseline_turn_keys=set(browser_baseline.turn_keys),
                    baseline_assistant_count=browser_baseline.assistant_count,
                    baseline_latest_assistant_key=browser_baseline.latest_assistant_key,
                    baseline_latest_assistant_hash=browser_baseline.latest_assistant_hash,
                )
                self.active_generation = tracker
                if not self._install_generation_observer(tracker):
                    self.state = AdapterState.DEGRADED
                    return self.advice_degraded(
                        validated.request_id,
                        "DeepSeek virtual-list observer could not be installed before submission.",
                    )
            elif self._live_browser_available():
                self.state = AdapterState.DEGRADED
                return self.advice_degraded(
                    validated.request_id,
                    "DeepSeek page was not a usable existing or empty chat before submission.",
                )
            baseline_texts = self._visible_response_texts(snapshot)
            baseline_user_count = self._visible_user_message_count(snapshot)
            baseline_conversation_count = self._visible_conversation_block_count(snapshot)
            url_path_before = self._snapshot_path(snapshot)
            self.state = AdapterState.SENDING
            self._type_prompt(prompt_selector, prompt)
            if self._live_browser_available() and not self._verify_prompt_ready_for_send(prompt):
                self.state = AdapterState.DEGRADED
                return self.advice_degraded(
                    validated.request_id,
                    "DeepSeek composer did not contain the intended prompt with an enabled Send control.",
                )
            if self.cancel_requested.is_set():
                self.state = AdapterState.CANCELLED
                return normalize_advice_response(
                    validated,
                    self.advisor_id,
                    AdvisorStatus.CANCELLED,
                    "DeepSeek advisory generation was cancelled.",
                    confidence="LOW",
                )
            clicked = (
                self._click_verified_composer_send()
                if self._live_browser_available()
                else self._click_first_enabled(self.selectors.send_button_selectors)
            )
            self._record_send_click_result(clicked)
            if not clicked:
                self.state = AdapterState.DEGRADED
                return self.advice_degraded(
                    validated.request_id,
                    "DeepSeek exact composer Send control was not clicked.",
                )
            submission_confirmed = (
                self._confirm_generation_submission(tracker, prompt_selector, url_path_before)
                if tracker is not None
                else self._confirm_submission(
                    prompt_selector,
                    baseline_user_count,
                    baseline_conversation_count,
                )
            )
            if not submission_confirmed:
                self.state = AdapterState.DEGRADED
                return self.advice_degraded(
                    validated.request_id,
                    "DeepSeek submission was not confirmed before waiting for a response.",
                )
            self.state = AdapterState.WAITING_FOR_RESPONSE
            if tracker is not None:
                return self._wait_for_generation_response(validated, tracker)
            return self._wait_for_response(validated, baseline_texts)
        except Exception as exc:
            sys.stderr.write(f"DeepSeek advisor operation failed: {type(exc).__name__}\n")
            self.state = AdapterState.DEGRADED
            return self.advice_failure(validated.request_id)
        finally:
            if self.active_generation is not None and self.state in {
                AdapterState.COMPLETED,
                AdapterState.CANCELLED,
                AdapterState.TIMED_OUT,
                AdapterState.RATE_LIMITED,
                AdapterState.TAKEOVER_REQUIRED,
                AdapterState.DEGRADED,
                AdapterState.FAILED,
            }:
                self._disconnect_generation_observers()
                self.active_generation = None

    def cancel(self, request_id: str | None = None) -> AdviceResponseV1:
        self.cancel_requested.set()
        if self.active_generation is not None:
            self.active_generation.cancellation_event.set()
            self.active_generation.terminal_state = AdapterState.CANCELLED.value
        if self.state in {AdapterState.SENDING, AdapterState.WAITING_FOR_RESPONSE}:
            self._try_click_stop()
        self._disconnect_generation_observers()
        self.state = AdapterState.CANCELLED
        return AdviceResponseV1(
            schema_version=SCHEMA_VERSION,
            request_id=request_id or "cancel",
            advisor_id=self.advisor_id,
            status=AdvisorStatus.CANCELLED.value,
            diagnosis="DeepSeek advisory generation was cancelled.",
            recommendations=[],
            risks=[],
            assumptions_or_questions=[],
            confidence="LOW",
            raw_artifact_reference=None,
        )

    def _wait_for_response(
        self,
        request: ValidatedAdviceRequest,
        baseline_texts: list[str],
    ) -> AdviceResponseV1:
        started_at = self._now()
        last_text = ""
        last_text_change_at = started_at
        stable_samples = 0
        while self._now() - started_at <= self.selectors.timeout_seconds:
            if self.cancel_requested.is_set():
                self._try_click_stop()
                self.state = AdapterState.CANCELLED
                return normalize_advice_response(
                    request,
                    self.advisor_id,
                    AdvisorStatus.CANCELLED,
                    "DeepSeek advisory generation was cancelled.",
                    confidence="LOW",
                )
            snapshot = self._snapshot()
            if snapshot.origin != self.expected_origin:
                self.state = AdapterState.DEGRADED
                return self.advice_unavailable(request.request_id)
            takeover, _selector = snapshot.any_selector(self.selectors.takeover_required_selectors)
            if takeover:
                self.state = AdapterState.TAKEOVER_REQUIRED
                return self.advice_unavailable(request.request_id)
            login_required, _selector = snapshot.any_selector(self.selectors.login_required_selectors)
            if login_required:
                self.state = AdapterState.LOGIN_REQUIRED
                return self.advice_unavailable(request.request_id)
            rate_limited, _selector = snapshot.any_selector(self.selectors.rate_limit_selectors)
            if rate_limited:
                self.state = AdapterState.RATE_LIMITED
                return self.advice_unavailable(request.request_id)

            response_texts = self._visible_response_texts(snapshot)
            raw_response_count = self._visible_raw_assistant_count(snapshot)
            newest_text = newest_new_response_text(response_texts, baseline_texts)
            if newest_text != last_text:
                last_text = newest_text
                last_text_change_at = self._now()
                stable_samples = 0
            elif newest_text:
                stable_samples += 1
            stable_for = self._now() - last_text_change_at
            stop_visible = self._any_visible(snapshot, self.selectors.stop_button_selectors)
            send_ready = self._first_enabled_visible_selector(
                snapshot,
                self.selectors.send_button_selectors,
            ) is not None
            if not send_ready and not self._any_visible(
                snapshot,
                self.selectors.send_button_selectors,
            ):
                send_ready = self._any_visible(snapshot, self.selectors.prompt_input_selectors)

            if raw_response_count > len(baseline_texts) and not newest_text and not stop_visible:
                self.state = AdapterState.FAILED
                return self.advice_unavailable(request.request_id)
            if (
                newest_text
                and not stop_visible
                and send_ready
                and stable_for >= self.selectors.text_stability_seconds
                and stable_samples >= max(1, self.selectors.stable_sample_count)
            ):
                self.state = AdapterState.COMPLETED
                return normalize_advice_response(
                    request,
                    self.advisor_id,
                    AdvisorStatus.COMPLETED,
                    newest_text,
                )
            self._sleep(0.25)
        self.state = AdapterState.TIMED_OUT
        return self.advice_unavailable(request.request_id)

    def _wait_for_generation_response(
        self,
        request: ValidatedAdviceRequest,
        tracker: GenerationTracker,
    ) -> AdviceResponseV1:
        started_at = self._now()
        stable_samples = 0
        last_hash = ""
        while self._now() - started_at <= self.selectors.timeout_seconds:
            if self.cancel_requested.is_set() or tracker.cancellation_event.is_set():
                self._try_click_stop()
                tracker.terminal_state = AdapterState.CANCELLED.value
                self.state = AdapterState.CANCELLED
                return normalize_advice_response(
                    request,
                    self.advisor_id,
                    AdvisorStatus.CANCELLED,
                    "DeepSeek advisory generation was cancelled.",
                    confidence="LOW",
                )
            snapshot = self._snapshot()
            if snapshot.origin != self.expected_origin:
                tracker.terminal_state = AdapterState.DEGRADED.value
                self.state = AdapterState.DEGRADED
                return self.advice_unavailable(request.request_id)
            takeover, _selector = snapshot.any_selector(self.selectors.takeover_required_selectors)
            if takeover:
                tracker.terminal_state = AdapterState.TAKEOVER_REQUIRED.value
                self.state = AdapterState.TAKEOVER_REQUIRED
                return self.advice_unavailable(request.request_id)
            login_required, _selector = snapshot.any_selector(self.selectors.login_required_selectors)
            if login_required:
                tracker.terminal_state = AdapterState.LOGIN_REQUIRED.value
                self.state = AdapterState.LOGIN_REQUIRED
                return self.advice_unavailable(request.request_id)
            rate_limited, _selector = snapshot.any_selector(self.selectors.rate_limit_selectors)
            if rate_limited:
                tracker.terminal_state = AdapterState.RATE_LIMITED.value
                self.state = AdapterState.RATE_LIMITED
                return self.advice_unavailable(request.request_id)

            state = self._generation_state()
            if not state.get("ok"):
                tracker.terminal_state = AdapterState.DEGRADED.value
                self.state = AdapterState.DEGRADED
                return self.advice_unavailable_with_tracker(
                    request.request_id,
                    tracker,
                    "DeepSeek generation observer state was unavailable.",
                )
            self._update_tracker_from_browser_state(tracker, state)
            text = normalize_response_text(str(state.get("assistantText") or ""))
            text_hash = sha256_text(text) if text else ""
            if text_hash and text_hash == last_hash:
                stable_samples += 1
            elif text_hash:
                last_hash = text_hash
                stable_samples = 1
            controls = state.get("controls") if isinstance(state.get("controls"), dict) else {}
            stop_visible = bool(controls.get("stopVisible"))
            assistant_action_row_visible = bool(state.get("assistantActionRowVisible"))
            composer_usable = self._composer_usable_for_completion(controls)
            generation_inactive_now = bool(
                not stop_visible and (assistant_action_row_visible or composer_usable)
            )
            tracker.generation_active_now = not generation_inactive_now
            if generation_inactive_now:
                tracker.generation_inactive_sample_count += 1
            else:
                tracker.generation_inactive_sample_count = 0
            stable_for = (
                self._now() - (tracker.last_text_change_at or started_at)
                if tracker.last_text_change_at is not None
                else 0.0
            )
            tracker.stable_duration_seconds = stable_for
            tracker.stable_sample_count = stable_samples
            completion = self._generation_completion_conditions(
                request,
                tracker,
                text,
                stable_for,
                stable_samples,
                stop_visible,
                composer_usable,
                assistant_action_row_visible,
            )
            tracker.completion_blockers = completion
            if all(completion.values()):
                tracker.stable_sample_count = stable_samples
                tracker.terminal_state = AdapterState.COMPLETED.value
                self.state = AdapterState.COMPLETED
                return normalize_advice_response(
                    request,
                    self.advisor_id,
                    AdvisorStatus.COMPLETED,
                    text,
                )
            self._sleep(0.5)
        tracker.terminal_state = AdapterState.TIMED_OUT.value
        self.state = AdapterState.TIMED_OUT
        return self.advice_unavailable_with_tracker(
            request.request_id,
            tracker,
            "DeepSeek generation did not reach a completed observed assistant response.",
        )

    def _composer_usable_for_completion(self, controls: dict[str, Any]) -> bool:
        if not bool(controls.get("composerFound")):
            return False
        if not bool(controls.get("composerVisible")):
            return False
        if not bool(controls.get("composerEnabled")):
            return False
        if bool(controls.get("composerReadOnly")):
            return False
        value_length = int(controls.get("composerValueLength") or 0)
        if value_length == 0:
            return True
        return bool(controls.get("sendEnabled"))

    def _generation_completion_conditions(
        self,
        request: ValidatedAdviceRequest,
        tracker: GenerationTracker,
        text: str,
        stable_for: float,
        stable_samples: int,
        stop_visible: bool,
        composer_usable: bool,
        assistant_action_row_visible: bool,
    ) -> dict[str, bool]:
        return {
            "current_user_turn_detected": tracker.saw_user_turn,
            "current_assistant_turn_detected": tracker.saw_assistant_turn,
            "assistant_text_non_empty": bool(text),
            "assistant_text_changed_after_submission": tracker.saw_assistant_text_change,
            "response_not_invalid": not self._response_is_invalid_for_generation(text, request, tracker),
            "stable_for_required_seconds": stable_for >= self.selectors.text_stability_seconds,
            "stable_hash_sample_count": stable_samples >= max(1, self.selectors.stable_sample_count),
            "generation_inactive_evidence": bool(
                assistant_action_row_visible
                or (
                    not stop_visible
                    and composer_usable
                    and tracker.generation_inactive_sample_count >= 3
                )
            ),
        }

    def _response_is_invalid_for_generation(
        self,
        text: str,
        request: ValidatedAdviceRequest,
        tracker: GenerationTracker,
    ) -> bool:
        normalized = normalize_response_text(text)
        if not normalized:
            return True
        if sha256_text(normalized) == tracker.baseline_latest_assistant_hash:
            return True
        if sha256_text(normalized) == tracker.submitted_prompt_hash:
            return True
        if normalized == normalize_response_text(request.specific_question):
            return True
        if tracker.detected_user_turn_key and tracker.detected_user_turn_key == tracker.detected_assistant_turn_key:
            return True
        return False

    def _live_browser_available(self) -> bool:
        if self._sb is None:
            return False
        return any(
            callable(getattr(self._sb, name, None))
            for name in ["execute_script", "get_page_source", "get_page_html"]
        ) or callable(getattr(getattr(self._sb, "cdp", None), "evaluate", None))

    def _browser_virtual_list_baseline(self) -> VirtualListBaseline | None:
        if not self._live_browser_available():
            return None
        value = self._execute_browser_script(DEEPSEEK_GENERATION_BOOTSTRAP_SCRIPT)
        if not isinstance(value, dict):
            return VirtualListBaseline(False, [], 0, None, None, [])
        if not value.get("rootFound"):
            return VirtualListBaseline(
                False,
                [],
                0,
                None,
                None,
                [],
                valid_empty_chat=bool(value.get("validEmptyChat")),
            )
        turns = [
            VirtualTurn(
                str(turn.get("key") or f"identity:{index}"),
                str(turn.get("role") or "other"),
                normalize_response_text(str(turn.get("text") or "")),
                sha256_text(normalize_response_text(str(turn.get("text") or ""))),
                int(turn.get("textLength") or 0),
            )
            for index, turn in enumerate(value.get("turns") or [])
            if isinstance(turn, dict)
        ]
        latest_text = normalize_response_text(str(value.get("latestAssistantText") or ""))
        return VirtualListBaseline(
            True,
            [str(key) for key in value.get("turnKeys") or []],
            int(value.get("assistantCount") or 0),
            str(value.get("latestAssistantKey")) if value.get("latestAssistantKey") else None,
            sha256_text(latest_text) if latest_text else None,
            turns,
            valid_empty_chat=bool(value.get("validEmptyChat")),
        )

    def _install_generation_observer(self, tracker: GenerationTracker) -> bool:
        result = self._execute_browser_script(
            f"{DEEPSEEK_GENERATION_INSTALL_SCRIPT}("
            f"{json.dumps(tracker.generation_id)},"
            f"{json.dumps(tracker.request_id)},"
            f"{json.dumps(tracker.submitted_prompt_hash)},"
            f"{json.dumps(sorted(tracker.baseline_turn_keys))},"
            f"{json.dumps(tracker.baseline_assistant_count)},"
            f"{json.dumps(tracker.baseline_latest_assistant_key)},"
            f"{json.dumps(tracker.baseline_latest_assistant_hash)},"
            f"{json.dumps(not tracker.baseline_turn_keys and tracker.baseline_assistant_count == 0)}"
            f")"
        )
        return isinstance(result, dict) and bool(result.get("ok"))

    def _generation_state(self) -> dict[str, Any]:
        value = self._execute_browser_script(DEEPSEEK_GENERATION_STATE_SCRIPT)
        return value if isinstance(value, dict) else {"ok": False, "error": "INVALID_STATE"}

    def _disconnect_generation_observers(self) -> None:
        with contextlib.suppress(Exception):
            self._execute_browser_script(DEEPSEEK_GENERATION_DISCONNECT_SCRIPT)

    def _update_tracker_from_browser_state(
        self,
        tracker: GenerationTracker,
        state: dict[str, Any],
    ) -> None:
        now = self._now()
        text = normalize_response_text(str(state.get("assistantText") or ""))
        text_hash = sha256_text(text) if text else ""
        previous_hash = tracker.assistant_text_hash
        tracker.detected_user_turn_key = (
            str(state.get("detectedUserTurnKey")) if state.get("detectedUserTurnKey") else None
        )
        tracker.detected_assistant_turn_key = (
            str(state.get("detectedAssistantTurnKey"))
            if state.get("detectedAssistantTurnKey")
            else None
        )
        tracker.assistant_element_identity = tracker.detected_assistant_turn_key
        tracker.assistant_text = text
        tracker.previous_text_hash = previous_hash
        tracker.assistant_text_hash = text_hash
        tracker.mutation_count = int(state.get("mutationCount") or 0)
        tracker.saw_user_turn = bool(state.get("sawUserTurn"))
        tracker.saw_assistant_turn = bool(state.get("sawAssistantTurn"))
        tracker.saw_generation_active = bool(state.get("sawGenerationActive"))
        tracker.assistant_action_row_visible = bool(state.get("assistantActionRowVisible"))
        controls = state.get("controls") if isinstance(state.get("controls"), dict) else {}
        tracker.composer_found = bool(controls.get("composerFound"))
        tracker.composer_visible = bool(controls.get("composerVisible"))
        tracker.composer_enabled = bool(controls.get("composerEnabled"))
        tracker.composer_read_only = bool(controls.get("composerReadOnly"))
        tracker.composer_value_length = int(controls.get("composerValueLength") or 0)
        tracker.send_visible = bool(controls.get("sendVisible"))
        tracker.send_enabled = bool(controls.get("sendEnabled"))
        tracker.stop_visible = bool(controls.get("stopVisible"))
        if text_hash and text_hash != previous_hash:
            tracker.saw_assistant_text_change = True
            tracker.last_text_change_at = now
            tracker.stable_sample_count = 0
        elif text_hash:
            tracker.stable_sample_count += 1
        if state.get("lastMutationAt"):
            tracker.last_mutation_at = now

    def _try_click_stop(self) -> None:
        try:
            self._click_first(self.selectors.stop_button_selectors)
        except Exception as exc:
            sys.stderr.write(f"DeepSeek advisor stop control failed: {type(exc).__name__}\n")

    def _snapshot_path(self, snapshot: PageSnapshot) -> str:
        return urlparse(snapshot.url).path

    def _confirm_submission(
        self,
        prompt_selector: str,
        baseline_user_count: int,
        baseline_conversation_count: int,
    ) -> bool:
        started_at = self._now()
        self.last_submission_diagnostics = {}
        while self._now() - started_at <= self.selectors.submission_confirmation_timeout_seconds:
            if self.cancel_requested.is_set():
                return True
            snapshot = self._snapshot()
            if snapshot.origin != self.expected_origin:
                self.last_submission_diagnostics = {"origin": snapshot.origin}
                return False
            user_count = self._visible_user_message_count(snapshot)
            conversation_count = self._visible_conversation_block_count(snapshot)
            if (
                user_count > baseline_user_count
                or conversation_count != baseline_conversation_count
            ):
                self.last_submission_diagnostics = {
                    "confirmed": True,
                    "user_count_before": baseline_user_count,
                    "user_count_after": user_count,
                    "conversation_count_before": baseline_conversation_count,
                    "conversation_count_after": conversation_count,
                }
                return True
            self.last_submission_diagnostics = {
                "confirmed": False,
                "user_count_before": baseline_user_count,
                "user_count_after": user_count,
                "conversation_count_before": baseline_conversation_count,
                "conversation_count_after": conversation_count,
                "origin": snapshot.origin,
            }
            self._sleep(0.25)
        return False

    def _confirm_generation_submission(
        self,
        tracker: GenerationTracker | None,
        prompt_selector: str,
        url_path_before: str = "",
    ) -> bool:
        if tracker is None:
            return False
        started_at = self._now()
        self.last_submission_diagnostics = {}
        while self._now() - started_at <= self.selectors.submission_confirmation_timeout_seconds:
            if self.cancel_requested.is_set():
                tracker.cancellation_event.set()
                return True
            snapshot = self._snapshot()
            if snapshot.origin != self.expected_origin:
                self.last_submission_diagnostics = {"origin": snapshot.origin}
                return False
            state = self._generation_state()
            if not state.get("ok"):
                self.last_submission_diagnostics = {
                    "confirmed": False,
                    "generation_id": tracker.generation_id,
                    "error": state.get("error"),
                }
                return False
            self._update_tracker_from_browser_state(tracker, state)
            controls = state.get("controls") if isinstance(state.get("controls"), dict) else {}
            url_path_after = str(state.get("urlPath") or self._snapshot_path(snapshot))
            root_count = int(state.get("rootCount") or (1 if state.get("rootFound") else 0))
            user_turn_count = int(state.get("userTurnCount") or 0)
            route_created = (
                url_path_after != url_path_before
                and bool(re.fullmatch(r"/a/chat/s/[A-Za-z0-9_-]+", url_path_after))
                and root_count > 0
            )
            confirmed = bool(
                tracker.saw_user_turn
                or user_turn_count > 0
                or route_created
            )
            self.last_submission_diagnostics = {
                "confirmed": confirmed,
                "generation_id": tracker.generation_id,
                "baseline_turn_count": len(tracker.baseline_turn_keys),
                "root_found": bool(state.get("rootFound")),
                "root_count": root_count,
                "user_turn_count": user_turn_count,
                "url_path_before": url_path_before,
                "url_path_after": url_path_after,
                "route_created": route_created,
                "lifecycle": state.get("lifecycle"),
                "saw_user_turn": tracker.saw_user_turn,
                "saw_assistant_turn": tracker.saw_assistant_turn,
                "saw_generation_active": tracker.saw_generation_active,
                "mutation_count": tracker.mutation_count,
                "bootstrap_mutation_count": tracker.mutation_count,
                "send_visible": bool(controls.get("sendVisible")),
                "send_enabled": bool(controls.get("sendEnabled")),
            }
            if confirmed:
                return True
            self._sleep(0.25)
        return False

    def _snapshot(self) -> PageSnapshot:
        if self._sb is None:
            raise RuntimeError("browser not started")
        with contextlib.redirect_stdout(sys.stderr):
            html = call_first(self._sb, ["get_page_source", "get_page_html"]) or ""
            title = call_first(self._sb, ["get_page_title", "get_title"]) or ""
            url = call_first(self._sb, ["get_current_url"]) or getattr(
                getattr(self._sb, "driver", None),
                "current_url",
                "",
            )
        return PageSnapshot(html=str(html), title=str(title), url=str(url))

    def _find_prompt_selector(self, snapshot: PageSnapshot) -> str | None:
        for selector in self.selectors.prompt_input_selectors:
            if snapshot.selector_is_visible(selector):
                return selector
        return None

    def _type_prompt(self, selector: str, prompt: str) -> None:
        if self._sb is None:
            raise RuntimeError("browser not started")
        with contextlib.redirect_stdout(sys.stderr):
            if hasattr(self._sb, "click"):
                self._sb.click(selector)
            self._clear_prompt(selector)
            existing = self._prompt_value(selector)
            if existing not in {None, ""}:
                raise RuntimeError("prompt input retained text after clear")
        self._type_text_paced(selector, prompt)

    def _verify_prompt_ready_for_send(self, prompt: str) -> bool:
        intended = normalize_response_text(prompt)
        state = self._deepseek_composer_send_state()
        dom_value = normalize_response_text(str(state.get("value") or ""))
        if bool(state.get("composerFound")) and dom_value != intended:
            self._set_exact_composer_value(prompt)
            state = self._deepseek_composer_send_state()
            dom_value = normalize_response_text(str(state.get("value") or ""))
        diagnostics = dict(self.last_submission_diagnostics)
        diagnostics.update(
            {
                "prompt_intended_length": len(intended),
                "prompt_intended_hash": sha256_text(intended),
                "prompt_dom_value_length": len(dom_value),
                "prompt_dom_value_hash": sha256_text(dom_value),
                "prompt_value_matches": dom_value == intended,
                "send_visible": bool(state.get("sendVisible")),
                "send_enabled": bool(state.get("sendEnabled")),
                "exact_click_attempted": False,
                "exact_click_returned": False,
                "url_path_before": str(state.get("urlPath") or ""),
                "url_path_after": str(state.get("urlPath") or ""),
                "conversation_root_count": int(state.get("rootCount") or 0),
                "user_turn_count": int(state.get("userTurnCount") or 0),
                "bootstrap_mutation_count": 0,
            }
        )
        self.last_submission_diagnostics = diagnostics
        return (
            bool(state.get("composerFound"))
            and dom_value == intended
            and bool(state.get("sendVisible"))
            and bool(state.get("sendEnabled"))
        )

    def _set_exact_composer_value(self, prompt: str) -> bool:
        script = f"""
(() => {{
  const element = document.querySelector('textarea[placeholder="Message DeepSeek"]');
  if (!element) return false;
  element.focus();
  element.value = {json.dumps(prompt)};
  element.dispatchEvent(new InputEvent("input", {{
    bubbles: true,
    cancelable: true,
    inputType: "insertText",
    data: null
  }}));
  element.dispatchEvent(new Event("change", {{ bubbles: true }}));
  return true;
}})()
"""
        with contextlib.suppress(Exception):
            return bool(self._execute_browser_script(script))
        return False

    def _record_send_click_result(self, clicked: bool) -> None:
        state = self._deepseek_composer_send_state()
        diagnostics = dict(self.last_submission_diagnostics)
        diagnostics.update(
            {
                "exact_click_attempted": True,
                "exact_click_returned": bool(clicked),
                "url_path_after": str(state.get("urlPath") or diagnostics.get("url_path_after") or ""),
                "conversation_root_count": int(state.get("rootCount") or 0),
                "user_turn_count": int(state.get("userTurnCount") or 0),
            }
        )
        self.last_submission_diagnostics = diagnostics

    def _type_text_paced(self, selector: str, text: str) -> None:
        if self._sb is None:
            raise RuntimeError("browser not started")
        started_at = self._now()
        minimum = max(0.0, self.selectors.typing_min_interval_seconds)
        maximum = max(minimum, self.selectors.typing_max_interval_seconds)
        newline_minimum = max(0.0, self.selectors.typing_newline_pause_min_seconds)
        newline_maximum = max(newline_minimum, self.selectors.typing_newline_pause_max_seconds)
        native_element = self._first_native_typing_element(selector)
        for char in text:
            if self.cancel_requested.is_set():
                return
            if self._now() - started_at > self.selectors.typing_timeout_seconds:
                raise TimeoutError("paced typing timed out")
            if not self._type_character_native(selector, char, native_element):
                self._insert_prompt_character(selector, char)
            delay = (
                self.rng.uniform(newline_minimum, newline_maximum)
                if char == "\n"
                else self.rng.uniform(minimum, maximum)
            )
            self._sleep(delay)

    def _first_native_typing_element(self, selector: str) -> Any | None:
        if self._sb is None:
            return None
        elements = self._find_elements(selector)
        return elements[0] if elements else None

    def _type_character_native(self, selector: str, char: str, element: Any | None = None) -> bool:
        if self._sb is None:
            return False
        if element is not None:
            method = getattr(element, "send_keys", None)
            if callable(method):
                with contextlib.suppress(Exception):
                    method(char)
                    return True
        with contextlib.redirect_stdout(sys.stderr):
            if hasattr(self._sb, "press_keys"):
                self._sb.press_keys(selector, char)
                return True
            if hasattr(self._sb, "type"):
                self._sb.type(selector, char)
                return True
        return False

    def _insert_prompt_character(self, selector: str, char: str) -> bool:
        script = f"""
(() => {{
  const element = document.querySelector({json.dumps(selector)});
  if (!element) return false;
  element.focus();
  const value = element.value ?? element.innerText ?? "";
  if (typeof element.setRangeText === "function") {{
    const start = element.selectionStart ?? value.length;
    const end = element.selectionEnd ?? start;
    element.setRangeText({json.dumps(char)}, start, end, "end");
  }} else if ("value" in element) {{
    element.value = value + {json.dumps(char)};
  }} else {{
    element.textContent = value + {json.dumps(char)};
  }}
  element.dispatchEvent(new InputEvent("input", {{
    bubbles: true,
    cancelable: true,
    inputType: {json.dumps("insertLineBreak" if char == "\n" else "insertText")},
    data: {json.dumps(None if char == "\n" else char)}
  }}));
  element.dispatchEvent(new Event("change", {{ bubbles: true }}));
  return true;
}})()
"""
        with contextlib.suppress(Exception):
            return bool(self._execute_browser_script(script))
        return False

    def _clear_prompt(self, selector: str) -> None:
        if self._sb is None:
            raise RuntimeError("browser not started")
        if hasattr(self._sb, "clear"):
            self._sb.clear(selector)
            return
        if hasattr(self._sb, "clear_text"):
            self._sb.clear_text(selector)
            return
        if hasattr(self._sb, "set_value"):
            self._sb.set_value(selector, "")
            return
        if hasattr(self._sb, "press_keys"):
            self._sb.press_keys(selector, "\ue009a")
            self._sb.press_keys(selector, "\ue003")

    def _prompt_value(self, selector: str) -> str | None:
        if self._sb is None:
            return None
        for method_name in ["get_attribute", "get_element_attribute"]:
            method = getattr(self._sb, method_name, None)
            if callable(method):
                with contextlib.suppress(Exception):
                    value = method(selector, "value")
                    if value is not None:
                        return str(value)
        return None

    def _prompt_is_empty(self, selector: str) -> bool:
        value = self._prompt_value(selector)
        if value is not None:
            return value == ""
        script = f"""
(() => {{
  const element = document.querySelector({json.dumps(selector)});
  if (!element) return false;
  return ((element.value || element.innerText || '').trim().length === 0);
}})()
"""
        return bool(self._execute_browser_script(script))

    def _press_enter(self, selector: str) -> None:
        if self._sb is None:
            return
        with contextlib.redirect_stdout(sys.stderr):
            if hasattr(self._sb, "press_keys"):
                self._sb.press_keys(selector, "\n")

    def _click_first(self, selectors: Iterable[str]) -> bool:
        return self._click_first_enabled(selectors)

    def _click_first_enabled(self, selectors: Iterable[str]) -> bool:
        if self._sb is None:
            return False
        snapshot = self._snapshot()
        for selector in selectors:
            if selector == DEEPSEEK_COMPOSER_SEND_SELECTOR:
                if self._click_verified_composer_send():
                    return True
                continue
            if (
                self._selector_is_visible(snapshot, selector)
                and self._selector_is_enabled(snapshot, selector)
                and hasattr(self._sb, "click")
            ):
                with contextlib.redirect_stdout(sys.stderr):
                    self._sb.click(selector)
                return True
        return False

    def _any_visible(self, snapshot: PageSnapshot, selectors: Iterable[str]) -> bool:
        return any(self._selector_is_visible(snapshot, selector) for selector in selectors)

    def _any_visible_selector(
        self,
        snapshot: PageSnapshot,
        selectors: Iterable[str],
    ) -> tuple[bool, str | None]:
        for selector in selectors:
            if self._selector_is_visible(snapshot, selector):
                return True, selector
        return False, None

    def _first_enabled_visible_selector(
        self,
        snapshot: PageSnapshot,
        selectors: Iterable[str],
    ) -> str | None:
        for selector in selectors:
            if self._selector_is_visible(snapshot, selector) and self._selector_is_enabled(
                snapshot,
                selector,
            ):
                return selector
        return None

    def _selector_is_visible(self, snapshot: PageSnapshot, selector: str) -> bool:
        if selector == DEEPSEEK_COMPOSER_SEND_SELECTOR:
            return bool(self._deepseek_composer_send_state().get("sendVisible"))
        if selector == DEEPSEEK_COMPOSER_STOP_SELECTOR:
            return False
        if self._sb is not None:
            method = getattr(self._sb, "is_element_visible", None)
            if callable(method):
                with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                    return bool(method(selector))
        return snapshot.selector_is_visible(selector)

    def _selector_is_enabled(self, snapshot: PageSnapshot, selector: str) -> bool:
        if selector == DEEPSEEK_COMPOSER_SEND_SELECTOR:
            return bool(self._deepseek_composer_send_state().get("sendEnabled"))
        if selector == DEEPSEEK_COMPOSER_STOP_SELECTOR:
            return False
        if self._sb is not None:
            method = getattr(self._sb, "is_element_enabled", None)
            if callable(method):
                with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                    return bool(method(selector))
        return snapshot.selector_is_enabled(selector)

    def _visible_response_texts(self, snapshot: PageSnapshot) -> list[str]:
        if self._sb is not None:
            texts: list[str] = []
            for selector in self.selectors.assistant_message_selectors:
                if selector == DEEPSEEK_CONVERSATION_BLOCKS_SELECTOR:
                    texts.extend(self._deepseek_visible_conversation_blocks(role="assistant"))
                    continue
                elements = self._find_elements(selector)
                for element in elements:
                    if self._element_is_displayed(element):
                        text = normalize_plain_text(str(getattr(element, "text", "") or ""))
                        if text:
                            texts.append(text)
            if texts:
                return filter_assistant_response_texts(texts, self._visible_user_message_texts(snapshot))
        return filter_assistant_response_texts(
            snapshot.response_texts(self.selectors.assistant_message_selectors),
            self._visible_user_message_texts(snapshot),
        )

    def _visible_raw_assistant_count(self, snapshot: PageSnapshot) -> int:
        if self._sb is not None:
            count = 0
            for selector in self.selectors.assistant_message_selectors:
                if selector == DEEPSEEK_CONVERSATION_BLOCKS_SELECTOR:
                    count += len(self._deepseek_visible_conversation_blocks(role="assistant"))
                    continue
                count += sum(
                    1 for element in self._find_elements(selector) if self._element_is_displayed(element)
                )
            if count:
                return count
        return len(snapshot.response_texts(self.selectors.assistant_message_selectors))

    def _visible_user_message_count(self, snapshot: PageSnapshot) -> int:
        return len(self._visible_user_message_texts(snapshot))

    def _visible_user_message_texts(self, snapshot: PageSnapshot) -> list[str]:
        if self._sb is not None:
            texts: list[str] = []
            for selector in self.selectors.user_message_selectors:
                if selector == DEEPSEEK_CONVERSATION_BLOCKS_SELECTOR:
                    texts.extend(self._deepseek_visible_conversation_blocks(role="user"))
                    continue
                for element in self._find_elements(selector):
                    if self._element_is_displayed(element):
                        text = normalize_plain_text(str(getattr(element, "text", "") or ""))
                        if text:
                            texts.append(text)
            if texts:
                return texts
        parser_selectors = [
            selector
            for selector in self.selectors.user_message_selectors
            if not selector.startswith("deepseek:")
        ]
        return snapshot.response_texts(parser_selectors)

    def _visible_conversation_block_count(self, snapshot: PageSnapshot) -> int:
        return len(self._visible_response_texts(snapshot)) + self._visible_user_message_count(snapshot)

    def _find_elements(self, selector: str) -> list[Any]:
        if self._sb is None:
            return []
        for method_name in ["find_elements", "find_visible_elements"]:
            method = getattr(self._sb, method_name, None)
            if callable(method):
                with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                    result = method(selector)
                    return list(result or [])
        driver = getattr(self._sb, "driver", None)
        method = getattr(driver, "find_elements", None)
        if callable(method):
            with contextlib.suppress(Exception):
                return list(method("css selector", selector) or [])
        return []

    def _element_is_displayed(self, element: Any) -> bool:
        method = getattr(element, "is_displayed", None)
        if callable(method):
            with contextlib.suppress(Exception):
                return bool(method())
        return True

    def _execute_browser_script(self, script: str) -> Any:
        if self._sb is None:
            return None
        cdp = getattr(self._sb, "cdp", None)
        method = getattr(cdp, "evaluate", None)
        if callable(method):
            with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                return method(script)
        method = getattr(self._sb, "execute_script", None)
        if callable(method):
            with contextlib.suppress(Exception), contextlib.redirect_stdout(sys.stderr):
                return method(script)
        driver = getattr(self._sb, "driver", None)
        method = getattr(driver, "execute_script", None)
        if callable(method):
            with contextlib.suppress(Exception):
                return method(script)
        return None

    def _deepseek_composer_send_state(self) -> dict[str, Any]:
        value = self._execute_browser_script(DEEPSEEK_COMPOSER_SEND_STATE_SCRIPT)
        if isinstance(value, dict):
            return value
        return {
            "composerFound": False,
            "value": "",
            "valueLength": 0,
            "sendVisible": False,
            "sendEnabled": False,
            "sendDisabled": False,
            "urlPath": "",
            "rootCount": 0,
            "userTurnCount": 0,
        }

    def _click_verified_composer_send(self) -> bool:
        value = self._execute_browser_script(DEEPSEEK_COMPOSER_SEND_CLICK_SCRIPT)
        return isinstance(value, dict) and bool(value.get("clicked"))

    def _deepseek_visible_conversation_blocks(self, *, role: str) -> list[str]:
        script = DEEPSEEK_VISIBLE_CONVERSATION_BLOCKS_SCRIPT.replace("__CATDESK_ROLE__", role)
        value = self._execute_browser_script(script)
        if not isinstance(value, list):
            return []
        return [
            normalize_plain_text(str(item))
            for item in value
            if normalize_plain_text(str(item))
        ]

    def _handle_cookie_banner(self, snapshot: PageSnapshot | None = None) -> bool:
        snapshot = snapshot or self._snapshot()
        if snapshot.origin != self.expected_origin:
            self.cookie_banner_result = "blocking"
            return False
        banner_visible = self._any_visible(snapshot, self.selectors.cookie_banner_selectors)
        if not banner_visible:
            self.cookie_banner_result = "absent"
            return False
        if self._click_first_enabled(self.selectors.cookie_reject_selectors):
            self.cookie_banner_result = "rejected"
            return True
        if self._click_first_enabled(self.selectors.cookie_accept_selectors):
            self.cookie_banner_result = "accepted_fallback"
            return True
        self.cookie_banner_result = "blocking"
        return False

    def _attempt_env_login(self) -> bool:
        email = os.environ.get("CATDESK_ADVISOR_DEEPSEEK_EMAIL", "")
        password = os.environ.get("CATDESK_ADVISOR_DEEPSEEK_PASSWORD", "")
        os.environ.pop("CATDESK_ADVISOR_DEEPSEEK_EMAIL", None)
        os.environ.pop("CATDESK_ADVISOR_DEEPSEEK_PASSWORD", None)
        if not email or not password:
            return False
        snapshot = self._snapshot()
        if snapshot.origin != self.expected_origin:
            return False
        if snapshot.any_selector(self.selectors.takeover_required_selectors)[0]:
            self.state = AdapterState.TAKEOVER_REQUIRED
            return False
        email_selector = self._first_enabled_visible_selector(
            snapshot,
            self.selectors.login_email_selectors,
        )
        password_selector = self._first_enabled_visible_selector(
            snapshot,
            self.selectors.login_password_selectors,
        )
        if email_selector is None or password_selector is None:
            return False
        self._type_login_field(email_selector, email)
        self._type_login_field(password_selector, password)
        return self._click_first_enabled(self.selectors.login_submit_selectors)

    def _type_login_field(self, selector: str, value: str) -> None:
        if self._sb is None:
            return
        with contextlib.redirect_stdout(sys.stderr):
            if hasattr(self._sb, "click"):
                self._sb.click(selector)
            self._clear_prompt(selector)
        self._type_text_paced(selector, value)

    def _ensure_chat_selected(self, chat_name: str) -> bool:
        snapshot = self._snapshot()
        if snapshot.origin != self.expected_origin:
            return False
        if self._visible_text_exact(self.selectors.current_chat_title_selectors, chat_name):
            return True
        if not self._click_first_enabled(self.selectors.chat_menu_button_selectors):
            return False
        return self._click_exact_text(self.selectors.chat_item_selectors, chat_name)

    def _visible_text_exact(self, selectors: Iterable[str], expected: str) -> bool:
        expected_normalized = normalize_plain_text(expected)
        for selector in selectors:
            for element in self._find_elements(selector):
                if self._element_is_displayed(element):
                    text = normalize_plain_text(str(getattr(element, "text", "") or ""))
                    if text == expected_normalized:
                        return True
        return False

    def _click_exact_text(self, selectors: Iterable[str], expected: str) -> bool:
        expected_normalized = normalize_plain_text(expected)
        if self._sb is None:
            return False
        for selector in selectors:
            for element in self._find_elements(selector):
                if not self._element_is_displayed(element):
                    continue
                text = normalize_plain_text(str(getattr(element, "text", "") or ""))
                if text == expected_normalized:
                    with contextlib.redirect_stdout(sys.stderr):
                        element.click()
                    return True
        return False

    def _now(self) -> float:
        return time.monotonic()

    def _sleep(self, seconds: float) -> None:
        time.sleep(seconds)

    def advice_unavailable(self, request_id: str) -> AdviceResponseV1:
        status = STATE_TO_STATUS.get(self.state, AdvisorStatus.UNAVAILABLE)
        return AdviceResponseV1(
            schema_version=1,
            request_id=request_id,
            advisor_id=self.advisor_id,
            status=status.value,
            diagnosis="DeepSeek web advisor is not ready for an advisory turn.",
            recommendations=[],
            risks=["No browser advice was obtained."],
            assumptions_or_questions=["User may need to complete manual login or verification."],
            confidence="LOW",
            raw_artifact_reference=None,
        )

    def advice_unavailable_with_tracker(
        self,
        request_id: str,
        tracker: GenerationTracker,
        diagnosis: str,
    ) -> AdviceResponseV1:
        status = STATE_TO_STATUS.get(self.state, AdvisorStatus.UNAVAILABLE)
        details = json.dumps(tracker.redacted_diagnostics(), sort_keys=True)
        return AdviceResponseV1(
            schema_version=SCHEMA_VERSION,
            request_id=request_id,
            advisor_id=self.advisor_id,
            status=status.value,
            diagnosis=bound_text(diagnosis, 512),
            recommendations=[],
            risks=["No browser advice was safely obtained."],
            assumptions_or_questions=[bound_text(redact(details), 1200)],
            confidence="LOW",
            raw_artifact_reference=None,
        )

    def advice_failure(self, request_id: str) -> AdviceResponseV1:
        return AdviceResponseV1(
            schema_version=SCHEMA_VERSION,
            request_id=request_id,
            advisor_id=self.advisor_id,
            status=AdvisorStatus.FAILED.value,
            diagnosis="DeepSeek web advisor failed during a bounded browser operation.",
            recommendations=[],
            risks=["No browser advice was safely obtained."],
            assumptions_or_questions=[],
            confidence="LOW",
            raw_artifact_reference=None,
        )

    def advice_degraded(self, request_id: str, diagnosis: str) -> AdviceResponseV1:
        details = json.dumps(self.last_submission_diagnostics, sort_keys=True)
        return AdviceResponseV1(
            schema_version=SCHEMA_VERSION,
            request_id=request_id,
            advisor_id=self.advisor_id,
            status=AdvisorStatus.DEGRADED.value,
            diagnosis=bound_text(diagnosis, 512),
            recommendations=[],
            risks=["No confirmed browser submission occurred."],
            assumptions_or_questions=[bound_text(redact(details), 1024)],
            confidence="LOW",
            raw_artifact_reference=None,
        )


class JsonLinesAdvisorProtocol:
    def __init__(self, adapter: DeepSeekWebAdvisorAdapter, auth_token: str) -> None:
        if not auth_token:
            raise ValueError("auth_token must be non-empty")
        self.adapter = adapter
        self.auth_token = auth_token
        self._lock = threading.Lock()
        self._active_thread: threading.Thread | None = None
        self._active_request_id: str | None = None
        self._cancelled_request_ids: set[str] = set()

    def handle(
        self,
        message: dict[str, Any],
        emit: Any | None = None,
    ) -> dict[str, Any]:
        if message.get("auth_token") != self.auth_token:
            return {"ok": False, "error": "UNAUTHORIZED"}
        command = message.get("command")
        if command == "hello":
            return {
                "ok": True,
                "advisor_id": self.adapter.advisor_id,
                "state": self.adapter.state.value,
                "protocol": "catdesk.advisor.deepseek.jsonl.v1",
            }
        if command == "status":
            with self._lock:
                active = self._thread_is_active_locked()
                request_id = self._active_request_id
            state = self.adapter.state.value if active else self.adapter.refresh_state().value
            return {
                "ok": True,
                "state": state,
                "active": active,
                "request_id": request_id,
            }
        if command == "start":
            return {"ok": True, "state": self.adapter.start().value}
        if command == "cancel":
            with self._lock:
                request_id = self._active_request_id
                if request_id is None or not self._thread_is_active_locked():
                    return {"ok": False, "error": "NO_ACTIVE_ADVICE"}
                self._cancelled_request_ids.add(request_id)
            response = self.adapter.cancel(request_id)
            return {
                "ok": True,
                "state": self.adapter.state.value,
                "request_id": request_id,
                "response": response.to_dict(),
            }
        if command == "shutdown":
            with self._lock:
                request_id = self._active_request_id
                if request_id is not None:
                    self._cancelled_request_ids.add(request_id)
            response = self.adapter.cancel(request_id)
            self._join_active(timeout=5.0)
            self.adapter.stop()
            return {"ok": True, "state": self.adapter.state.value, "response": response.to_dict()}
        if command == "advise":
            request = message.get("request") or {}
            try:
                validated = validate_advice_request(request)
            except ValueError as exc:
                return {"ok": False, "error": str(exc)}
            with self._lock:
                if self._thread_is_active_locked():
                    return {
                        "ok": False,
                        "error": "ADVICE_ALREADY_ACTIVE",
                        "request_id": self._active_request_id,
                    }
                state = self.adapter.refresh_state()
                if state != AdapterState.READY:
                    return {
                        "ok": False,
                        "error": f"ADVISOR_NOT_READY:{state.value}",
                        "state": state.value,
                    }
                self._active_request_id = validated.request_id
                self._cancelled_request_ids.discard(validated.request_id)
                thread = threading.Thread(
                    target=self._run_advice_worker,
                    args=(request, validated.request_id, emit),
                    name=f"deepseek-advice-{validated.request_id}",
                    daemon=True,
                )
                self._active_thread = thread
                thread.start()
            return {
                "ok": True,
                "accepted": True,
                "request_id": validated.request_id,
                "state": self.adapter.state.value,
            }
        return {"ok": False, "error": "UNKNOWN_COMMAND"}

    def _thread_is_active_locked(self) -> bool:
        return self._active_thread is not None and self._active_thread.is_alive()

    def _join_active(self, timeout: float) -> None:
        with self._lock:
            thread = self._active_thread
        if thread is not None:
            thread.join(timeout=timeout)

    def _run_advice_worker(
        self,
        request: dict[str, Any],
        request_id: str,
        emit: Any | None,
    ) -> None:
        try:
            response = self.adapter.advise(request)
            event = {
                "ok": True,
                "event": "advice_completed",
                "request_id": request_id,
                "response": response.to_dict(),
            }
        except Exception as exc:
            event = {
                "ok": False,
                "event": "advice_failed",
                "request_id": request_id,
                "error": f"FAILED:{type(exc).__name__}",
            }
        finally:
            with self._lock:
                was_cancelled = request_id in self._cancelled_request_ids
                self._cancelled_request_ids.discard(request_id)
                self._active_request_id = None
                self._active_thread = None
        if was_cancelled:
            return
        if emit is not None:
            emit(event)

    def serve(self, input_stream: TextIO, output_stream: TextIO) -> None:
        output_lock = threading.Lock()

        def emit(event: dict[str, Any]) -> None:
            with output_lock:
                output_stream.write(json.dumps(event, separators=(",", ":")) + "\n")
                output_stream.flush()

        for line in input_stream:
            if not line.strip():
                continue
            try:
                message = json.loads(line)
                with contextlib.redirect_stdout(sys.stderr):
                    response = self.handle(message, emit)
            except ValueError as exc:
                response = {"ok": False, "error": str(exc)}
            except Exception as exc:
                response = {"ok": False, "error": f"FAILED:{type(exc).__name__}"}
            emit(response)
        self._join_active(timeout=5.0)


def contains_prohibited_authority(value: Any) -> str | None:
    if isinstance(value, dict):
        for key, nested in value.items():
            if normalize_key(str(key)) in {normalize_key(item) for item in PROHIBITED_AUTHORITY_KEYS}:
                return str(key)
            found = contains_prohibited_authority(nested)
            if found:
                return found
    elif isinstance(value, list):
        for nested in value:
            found = contains_prohibited_authority(nested)
            if found:
                return found
    return None


def call_first(target: Any, names: Iterable[str]) -> Any:
    for name in names:
        method = getattr(target, name, None)
        if callable(method):
            return method()
    return None


def normalize_plain_text(value: str) -> str:
    return re.sub(r"\s+", " ", value).strip()


def normalize_response_text(value: str) -> str:
    text = value.replace("\r\n", "\n").replace("\r", "\n")
    lines = [re.sub(r"[ \t]+$", "", line) for line in text.split("\n")]
    text = "\n".join(lines)
    text = re.sub(r"\n{3,}", "\n\n", text)
    return text.strip()


def sha256_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def parse_simple_dom(html: str) -> SimpleDomNode:
    parser = SimpleDomParser()
    parser.feed(html)
    return parser.root


def collect_virtual_list_baseline(snapshot: PageSnapshot) -> VirtualListBaseline:
    root = parse_simple_dom(snapshot.html)
    virtual_root = next(
        (node for node in root.descendants() if node.has_class("ds-virtual-list-visible-items")),
        None,
    )
    if virtual_root is None:
        return VirtualListBaseline(False, [], 0, None, None, [])
    turns = []
    for index, child in enumerate([node for node in virtual_root.children if node.visible()]):
        turn = classify_virtual_turn(child, index)
        if turn.role != "other":
            turns.append(turn)
    assistants = [turn for turn in turns if turn.role == "assistant"]
    latest = assistants[-1] if assistants else None
    return VirtualListBaseline(
        True,
        [turn.key for turn in turns],
        len(assistants),
        latest.key if latest else None,
        latest.assistant_hash if latest else None,
        turns,
    )


def classify_virtual_turn(node: SimpleDomNode, index: int) -> VirtualTurn:
    key = node.attrs.get("data-virtual-list-item-key") or f"identity:{index}"
    assistant_node = node.first_descendant_with_classes(
        {"ds-markdown", "ds-assistant-message-main-content"}
    )
    if assistant_node is not None:
        text = normalize_response_text(assistant_node.text(preserve_lines=True))
        return VirtualTurn(key, "assistant", text, sha256_text(text), len(text))
    if node.has_descendant_with_class("ds-message"):
        return VirtualTurn(key, "user", "", "", 0)
    return VirtualTurn(key, "other", "", "", 0)


def newest_assistant_after_baseline(
    baseline: VirtualListBaseline,
    current: VirtualListBaseline,
) -> VirtualTurn | None:
    baseline_keys = set(baseline.turn_keys)
    candidates = [
        turn
        for turn in current.turns
        if turn.role == "assistant"
        and turn.key not in baseline_keys
        and turn.key != baseline.latest_assistant_key
        and turn.assistant_text
    ]
    if candidates:
        return candidates[-1]
    for turn in reversed(current.turns):
        if (
            turn.role == "assistant"
            and turn.key not in baseline_keys
            and turn.key != baseline.latest_assistant_key
            and turn.assistant_text
            and turn.assistant_hash != baseline.latest_assistant_hash
        ):
            return turn
    return None


def newest_new_response_text(
    response_texts: list[str],
    baseline_texts: list[str],
) -> str:
    normalized_baseline = [normalize_plain_text(text) for text in baseline_texts]
    normalized_responses = [normalize_plain_text(text) for text in response_texts]
    if len(normalized_responses) <= len(normalized_baseline):
        return ""
    newest = normalized_responses[-1]
    if not newest:
        return ""
    if newest in normalized_baseline:
        return ""
    return newest


def filter_assistant_response_texts(
    response_texts: list[str],
    user_texts: list[str],
) -> list[str]:
    normalized_users = [normalize_plain_text(text) for text in user_texts if normalize_plain_text(text)]
    filtered = []
    for text in response_texts:
        normalized = normalize_plain_text(text)
        if not normalized:
            continue
        if any(user and user in normalized for user in normalized_users):
            continue
        filtered.append(normalized)
    return filtered


def redact(value: str) -> str:
    redacted = value
    for pattern in SECRET_PATTERNS:
        redacted = pattern.sub("<redacted>", redacted)
    return redacted


def bound_text(value: str, max_bytes: int) -> str:
    if len(value.encode("utf-8")) <= max_bytes:
        return value
    suffix = "\n[truncated]"
    budget = max(0, max_bytes - len(suffix.encode("utf-8")))
    output = ""
    for char in value:
        if len((output + char).encode("utf-8")) > budget:
            break
        output += char
    return output + suffix if budget else suffix[:max_bytes]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile-dir", required=True, type=Path)
    parser.add_argument(
        "--selectors",
        default=Path(__file__).with_name("deepseek_selectors.json"),
        type=Path,
    )
    parser.add_argument(
        "--unsafe-headless-dev",
        action="store_true",
        help="Development only; manual login and security flows must not run headlessly.",
    )
    parser.add_argument(
        "--allow-env-login",
        action="store_true",
        help="Use CATDESK_ADVISOR_DEEPSEEK_EMAIL/PASSWORD for explicit opt-in login.",
    )
    parser.add_argument(
        "--chat-name",
        default=None,
        help="Optional exact chat name to select through configured chat selectors.",
    )
    args = parser.parse_args(argv)

    auth_token = os.environ.get("CATDESK_ADVISOR_AUTH_TOKEN", "")
    if not auth_token:
        sys.stderr.write("CATDESK_ADVISOR_AUTH_TOKEN is required\n")
        return 2
    selectors = SelectorConfig.from_file(args.selectors)
    adapter = DeepSeekWebAdvisorAdapter(
        selectors,
        args.profile_dir,
        headed=not args.unsafe_headless_dev,
        allow_env_login=args.allow_env_login,
        chat_name=args.chat_name,
    )
    JsonLinesAdvisorProtocol(adapter, auth_token).serve(sys.stdin, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
