"use strict";

const POLL_INTERVAL_MS = 3000;
const MAX_BACKOFF_MS = 30000;
const state = {
  snapshot: null, control: null, activeView: "overview", query: "", inFlight: null,
  pollTimer: null, failures: 0, adminLoaded: false, csrf: "", configOverrides: new Map(),
  configDirty: false, dialog: null,
  alertSticky: false,
};
const el = (id) => document.getElementById(id);
const ui = {
  liveIndicator: el("live-indicator"), connectionLabel: el("connection-label"), adminName: el("admin-name"),
  refreshButton: el("refresh-button"), reloadButton: el("reload-button"), searchInput: el("search-input"),
  alert: el("alert"), alertTitle: el("alert-title"), alertMessage: el("alert-message"),
  alertAction: el("alert-action"), loading: el("loading-state"), footerStatus: el("footer-status"),
  configGroups: el("config-groups"), configSave: el("config-save"), configResetAll: el("config-reset-all"),
  configDirty: el("config-dirty"), diagnosticGrid: el("diagnostic-grid"), listDialog: el("list-dialog"),
  listForm: el("list-form"), listValue: el("list-value"), listError: el("list-error"),
};

function setText(id, value) { const node = el(id); if (node) node.textContent = String(value); }
function number(value) { return new Intl.NumberFormat("zh-CN").format(Number(value) || 0); }
function formatDuration(seconds) {
  const total = Math.max(0, Math.floor(Number(seconds) || 0));
  const days = Math.floor(total / 86400); const hours = Math.floor((total % 86400) / 3600); const minutes = Math.floor((total % 3600) / 60);
  if (days > 0) return `${days}天 ${hours}时`; if (hours > 0) return `${hours}时 ${minutes}分`; return `${minutes}分`;
}
function formatClock(timestamp) {
  const date = new Date(timestamp); if (Number.isNaN(date.getTime())) return "--";
  return new Intl.DateTimeFormat("zh-CN", { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false }).format(date);
}
function formatRemaining(expiresAt) {
  const seconds = Math.max(0, Math.ceil((expiresAt - Date.now()) / 1000));
  if (seconds >= 3600) return `${Math.floor(seconds / 3600)}时 ${Math.floor((seconds % 3600) / 60)}分`;
  if (seconds >= 60) return `${Math.floor(seconds / 60)}分 ${seconds % 60}秒`; return `${seconds}秒`;
}
function matchesQuery(values) { return !state.query || values.some((value) => String(value).toLocaleLowerCase().includes(state.query)); }
function td(value, className) {
  const cell = document.createElement("td"); cell.textContent = String(value); if (className) cell.className = className; cell.title = String(value); return cell;
}
function remainingCell(expiresAt) { const cell = td(formatRemaining(expiresAt), "align-right remaining"); cell.dataset.expiresAt = String(expiresAt); return cell; }
function emptyRow(columnCount, message) { const row = document.createElement("tr"); row.className = "empty-row"; const cell = td(message); cell.colSpan = columnCount; row.append(cell); return row; }
function replaceRows(bodyId, rows, columnCount, emptyMessage) { const body = el(bodyId); if (body) body.replaceChildren(...(rows.length ? rows : [emptyRow(columnCount, emptyMessage)])); }
function commandButton(label, className, attributes) {
  const button = document.createElement("button"); button.type = "button"; button.textContent = label; button.className = className;
  Object.entries(attributes).forEach(([key, value]) => { button.dataset[key] = value; }); return button;
}

function cooldownReason(reason) {
  return ({
    repeated_upstream_auth_failure: "一分钟内多次鉴权失败",
    rate_limited: "上游 429 限流",
    capacity: "上游容量不足",
    bad_gateway: "上游 502 或连接失败",
  })[reason] || reason || "未记录";
}
function cooldownActionCell(kind, id, label) {
  const cell = document.createElement("td"); cell.className = "align-right";
  const button = commandButton("解除", "row-button is-danger", { clearCooldown: kind, id, label });
  cell.append(button); return cell;
}
function authCooldownRows(items, limit, includeAction = false) {
  return items.filter((item) => matchesQuery([item.scope, item.identifier, item.status, item.reason, cooldownReason(item.reason)])).slice(0, limit).map((item) => {
    const row = document.createElement("tr");
    row.append(td(item.scope === "api_key" ? "API Key" : "客户端 IP", "muted"), td(item.identifier, "mono"), td(cooldownReason(item.reason), "muted"), td(item.status, "status-code"), remainingCell(item.expiresAt));
    if (includeAction) row.append(cooldownActionCell(item.scope === "api_key" ? "auth_api_key" : "auth_client_ip", item.id, item.identifier));
    return row;
  });
}
function actionCell(kind, id, label, currentValue) {
  const cell = document.createElement("td"); cell.className = "align-right";
  const actions = document.createElement("div"); actions.className = "row-actions";
  const edit = commandButton("修改", "row-button", { editList: kind, id, label });
  if (currentValue) edit.dataset.currentValue = currentValue;
  const remove = commandButton("删除", "row-button is-danger", { deleteList: kind, id, label });
  actions.append(edit, remove); cell.append(actions); return cell;
}
function accessKeyRows(items, policy, kind) {
  return items.filter((item) => matchesQuery([item.masked, item.fingerprint])).map((item) => {
    const row = document.createElement("tr");
    row.append(td(item.masked, "mono"), td(item.fingerprint, "mono muted"), td(policy === "deny" ? "阻断" : "信任", `policy-${policy}`), actionCell(kind, item.id, item.masked));
    return row;
  });
}
function withExpiries(snapshot) {
  const receivedAt = Date.now();
  snapshot.auth_cooldowns.cooldowns = snapshot.auth_cooldowns.cooldowns.map((item) => ({ ...item, expiresAt: receivedAt + Number(item.remaining_secs || 0) * 1000 }));
  snapshot.retry_cooldowns.entries = snapshot.retry_cooldowns.entries.map((item) => ({ ...item, expiresAt: receivedAt + Number(item.remaining_secs || 0) * 1000 }));
  return snapshot;
}

function renderMetrics(snapshot) {
  const active = snapshot.concurrency.active; const blackKeys = snapshot.blacklist.total_api_keys; const blackIps = snapshot.blacklist.total_client_ips; const obs = snapshot.observability;
  setText("metric-active", number(active)); setText("metric-tracked", `跟踪 ${number(snapshot.concurrency.tracked_keys)} 个身份`);
  setText("metric-auth", number(snapshot.auth_cooldowns.total)); setText("metric-retry", number(snapshot.retry_cooldowns.total));
  setText("metric-blacklist", number(blackKeys + blackIps)); setText("metric-blacklist-note", `Key ${number(blackKeys)} · IP ${number(blackIps)}`);
  setText("metric-whitelist", number(snapshot.whitelist.total_api_keys)); setText("metric-requests", number(obs.requests_total));
  setText("metric-errors", `4xx ${number(obs.responses_4xx)} · 5xx ${number(obs.responses_5xx)}`); setText("load-active", number(active));
  setText("fact-tracked", number(snapshot.concurrency.tracked_keys)); setText("fact-cooldowns", number(snapshot.auth_cooldowns.total + snapshot.retry_cooldowns.total));
  setText("fact-generated", formatClock(snapshot.generated_at_ms)); setText("uptime-label", `运行 ${formatDuration(snapshot.uptime_secs)}`);
  const level = active === 0 ? 0 : active <= 3 ? 1 : active <= 10 ? 2 : active <= 25 ? 3 : 4;
  el("load-bar").className = `level-${level}`; el("load-meter").setAttribute("aria-valuenow", String(active));
}
function renderOverview(snapshot) { replaceRows("overview-auth-body", authCooldownRows(snapshot.auth_cooldowns.cooldowns, 6), 5, "当前没有 403 冷却"); }
function renderAccess(snapshot) {
  setText("black-key-count", snapshot.blacklist.api_keys_truncated ? `${number(snapshot.blacklist.total_api_keys)}+` : number(snapshot.blacklist.total_api_keys));
  setText("black-ip-count", snapshot.blacklist.client_ips_truncated ? `${number(snapshot.blacklist.total_client_ips)}+` : number(snapshot.blacklist.total_client_ips));
  setText("white-key-count", snapshot.whitelist.api_keys_truncated ? `${number(snapshot.whitelist.total_api_keys)}+` : number(snapshot.whitelist.total_api_keys));
  replaceRows("black-key-body", accessKeyRows(snapshot.blacklist.api_keys, "deny", "blacklist_key"), 4, "没有黑名单 Key");
  const ipRows = snapshot.blacklist.client_ips.filter((ip) => matchesQuery([ip])).map((ip) => {
    const row = document.createElement("tr"); row.append(td(ip, "mono"), td(ip.includes(":") ? "IPv6" : "IPv4", "muted"), td("阻断", "policy-deny"), actionCell("blacklist_ip", ip, ip, ip)); return row;
  });
  replaceRows("black-ip-body", ipRows, 4, "没有黑名单 IP"); replaceRows("white-key-body", accessKeyRows(snapshot.whitelist.api_keys, "trust", "whitelist_key"), 4, "没有白名单 Key");
}
function renderCooldowns(snapshot) {
  setText("auth-cooldown-count", snapshot.auth_cooldowns.truncated ? `${number(snapshot.auth_cooldowns.total)}+` : number(snapshot.auth_cooldowns.total));
  setText("retry-cooldown-count", snapshot.retry_cooldowns.truncated ? `${number(snapshot.retry_cooldowns.total)}+` : number(snapshot.retry_cooldowns.total));
  replaceRows("auth-cooldown-body", authCooldownRows(snapshot.auth_cooldowns.cooldowns, Infinity, true), 6, "当前没有 403 鉴权冷却");
  const retryRows = snapshot.retry_cooldowns.entries.filter((item) => matchesQuery([item.fingerprint, item.reason, cooldownReason(item.reason), "upstream"])).map((item) => {
    const row = document.createElement("tr"); row.append(td(item.fingerprint, "mono"), td(cooldownReason(item.reason), "muted"), td("上游异常", "muted"), remainingCell(item.expiresAt), cooldownActionCell("retry", item.id, item.fingerprint)); return row;
  });
  replaceRows("retry-cooldown-body", retryRows, 5, "当前没有重试冷却");
}
function renderLogs(snapshot) {
  const obs = snapshot.observability;
  setText("obs-requests", number(obs.requests_total)); setText("obs-2xx", number(obs.responses_2xx)); setText("obs-4xx", number(obs.responses_4xx));
  setText("obs-5xx", number(obs.responses_5xx)); setText("obs-retries", number(obs.upstream_retries)); setText("obs-mutations", number(obs.admin_mutations));
  setText("event-capacity", `保留上限 ${number(obs.event_capacity)}`);
  const rows = obs.events.filter((item) => matchesQuery([item.level, item.event, ...Object.values(item.fields)])).map((item) => {
    const row = document.createElement("tr"); const detail = Object.entries(item.fields).map(([key, value]) => `${key}=${value ?? "-"}`).join(" · ");
    row.append(td(formatClock(item.timestamp_ms), "mono muted"), td(item.level, `log-level level-${item.level}`), td(item.event, "mono"), td(detail, "mono muted log-detail")); return row;
  });
  replaceRows("event-log-body", rows, 4, "暂无运行日志");
}
function render() { if (state.snapshot) { renderMetrics(state.snapshot); renderOverview(state.snapshot); renderAccess(state.snapshot); renderCooldowns(state.snapshot); renderLogs(state.snapshot); } }

function renderControl(control, resetDraft) {
  state.control = control;
  if (resetDraft) {
    state.configOverrides = new Map(control.fields.filter((field) => field.overridden).map((field) => [field.key, field.value]));
    setConfigDirty(false);
  }
  setText("override-file-label", control.override_file.issue ? `覆盖文件异常 · ${control.override_file.path}` : `${control.override_file.overridden_fields} 项覆盖 · ${control.override_file.path}`);
  const names = { blacklist_key: "Key 黑名单", blacklist_ip: "IP 黑名单", whitelist_key: "Key 白名单", confirmation_rules: "确认规则" };
  ui.diagnosticGrid.replaceChildren(...control.files.map((file) => {
    const item = document.createElement("article"); item.className = `diagnostic-item is-${file.status}`;
    const title = document.createElement("div"); title.className = "diagnostic-name";
    const name = document.createElement("span"); name.textContent = names[file.kind] || file.kind;
    const health = document.createElement("span"); health.className = "health-mark"; health.textContent = file.status === "error" ? "异常" : file.status === "missing" ? "未创建" : "正常"; title.append(name, health);
    const path = document.createElement("p"); path.className = "diagnostic-path"; path.textContent = file.path; path.title = file.path;
    const meta = document.createElement("p"); meta.className = "diagnostic-meta"; meta.textContent = `缓存 ${number(file.cached_entries)} · 文件 ${file.exists ? number(file.size_bytes) + " B" : "不存在"}`;
    item.append(title, path, meta); if (file.issue) { const issue = document.createElement("p"); issue.className = "diagnostic-issue"; issue.textContent = file.issue; item.append(issue); } return item;
  }));
  if (control.override_file.issue) showAlert("管理员覆盖文件异常", control.override_file.issue, "重试", true);
  renderConfigFields();
}
function renderConfigFields() {
  if (!state.control) return;
  const groups = new Map(); state.control.fields.forEach((field) => { if (!groups.has(field.group)) groups.set(field.group, []); groups.get(field.group).push(field); });
  ui.configGroups.replaceChildren(...Array.from(groups, ([name, fields]) => {
    const section = document.createElement("section"); section.className = "config-group";
    const title = document.createElement("h3"); title.className = "config-group-title"; title.textContent = name;
    const container = document.createElement("div"); container.className = "config-fields"; container.append(...fields.map(configField)); section.append(title, container); return section;
  }));
}
function configField(field) {
  const wrapper = document.createElement("label"); wrapper.className = "config-field";
  const label = document.createElement("span"); label.className = "config-field-label"; const strong = document.createElement("strong"); strong.textContent = field.label;
  const code = document.createElement("code"); code.textContent = field.key; code.title = field.key; label.append(strong, code);
  let input;
  if (field.kind === "select" || field.kind === "boolean") {
    input = document.createElement("select"); input.className = "field-select";
    field.options.forEach((option) => {
      const node = document.createElement("option"); node.value = option;
      const policyLabels = { off: "关闭（不累计）", conservative: "保守（仅明确 401）", balanced: "平衡（明确 401/403）", strict: "严格（全部 New API 401/403）" };
      node.textContent = option === "true" ? "开启" : option === "false" ? "关闭" : field.key === "AI_BRIDGE_AUTH_FAILURE_POLICY" ? policyLabels[option] : option; input.append(node);
    });
  } else if (field.kind === "json") {
    input = document.createElement("textarea"); input.className = "field-textarea mono"; input.spellcheck = false;
  } else {
    input = document.createElement("input"); input.className = "field-input"; input.type = field.kind === "number" ? "number" : "text"; if (field.kind === "number") { input.min = field.min ?? ""; input.max = field.max ?? ""; }
  }
  input.value = state.configOverrides.has(field.key) ? state.configOverrides.get(field.key) : field.value; input.dataset.configKey = field.key;
  const reset = document.createElement("button"); reset.type = "button"; reset.className = `field-reset${state.configOverrides.has(field.key) ? " is-overridden" : ""}`;
  reset.textContent = state.configOverrides.has(field.key) ? "恢复" : "来源"; reset.title = state.configOverrides.has(field.key) ? "恢复到 .env 或进程环境值" : "当前来自 .env、进程环境或默认值";
  reset.disabled = !state.configOverrides.has(field.key); reset.addEventListener("click", () => { state.configOverrides.delete(field.key); setConfigDirty(true); renderConfigFields(); });
  input.addEventListener("input", () => { state.configOverrides.set(field.key, input.value); reset.disabled = false; reset.classList.add("is-overridden"); reset.textContent = "恢复"; setConfigDirty(true); });
  wrapper.append(label, input, reset); return wrapper;
}
function setConfigDirty(dirty) { state.configDirty = dirty; ui.configDirty.hidden = !dirty; ui.configSave.disabled = !dirty; }

function setView(view) {
  state.activeView = view;
  document.querySelectorAll("[role='tab']").forEach((tab) => { const active = tab.dataset.view === view; tab.classList.toggle("is-active", active); tab.setAttribute("aria-selected", String(active)); });
  document.querySelectorAll("[role='tabpanel']").forEach((panel) => { const active = panel.id === `panel-${view}`; panel.hidden = !active; panel.classList.toggle("is-active", active); });
}
function setConnection(mode, label) { ui.liveIndicator.className = `live-indicator ${mode === "live" ? "is-live" : mode === "error" ? "is-error" : ""}`; ui.connectionLabel.textContent = label; }
function showAlert(title, message, actionLabel, sticky = false) { state.alertSticky = sticky; ui.alert.hidden = false; ui.alertTitle.textContent = title; ui.alertMessage.textContent = message; ui.alertAction.textContent = actionLabel; }
function hideAlert(force = false) { if (state.alertSticky && !force) return; state.alertSticky = false; ui.alert.hidden = true; }
async function fetchJson(url, options = {}) {
  const headers = { Accept: "application/json", ...(options.body ? { "Content-Type": "application/json" } : {}), ...(options.mutate ? { "X-AI-Bridge-CSRF": state.csrf } : {}) };
  const response = await fetch(url, { credentials: "same-origin", cache: "no-store", ...options, headers: { ...headers, ...(options.headers || {}) } });
  let payload = null; try { payload = await response.json(); } catch (_) { payload = null; }
  if (!response.ok) { const error = new Error(payload?.error?.message || `HTTP ${response.status}`); error.status = response.status; error.payload = payload; throw error; }
  return payload;
}
function scheduleNext(delay) { window.clearTimeout(state.pollTimer); if (!document.hidden) state.pollTimer = window.setTimeout(() => refresh(false), delay); }
async function refreshAfterMutation(reloadControl = false) {
  if (reloadControl) state.control = null;
  if (state.inFlight) { state.inFlight.abort(); state.inFlight = null; }
  await refresh(true);
}

async function refresh(manual) {
  if (document.hidden) return;
  if (state.inFlight) {
    if (!manual) return;
    state.inFlight.abort();
    state.inFlight = null;
  }
  const controller = new AbortController(); state.inFlight = controller; ui.refreshButton.disabled = true; ui.refreshButton.classList.add("is-loading");
  if (!state.snapshot) ui.loading.classList.add("is-visible"); if (manual) setConnection("pending", "刷新中");
  try {
    if (!state.adminLoaded) { const identity = await fetchJson("/ai-bridge/api/me", { signal: controller.signal }); ui.adminName.textContent = identity.username || "管理员"; state.csrf = identity.csrf_token; state.adminLoaded = true; }
    const requests = [fetchJson("/ai-bridge/api/snapshot", { signal: controller.signal })]; if (!state.control) requests.push(fetchJson("/ai-bridge/api/control", { signal: controller.signal }));
    const [snapshot, control] = await Promise.all(requests); state.snapshot = withExpiries(snapshot); if (control) renderControl(control, true); state.failures = 0; render(); hideAlert();
    ui.loading.classList.remove("is-visible"); setConnection("live", "实时"); ui.footerStatus.textContent = `最后更新 ${formatClock(snapshot.generated_at_ms)}`; scheduleNext(POLL_INTERVAL_MS);
  } catch (error) {
    if (error.name === "AbortError") return;
    state.failures += 1; const unauthorized = error.status === 401 || error.status === 403; const backoff = Math.min(MAX_BACKOFF_MS, POLL_INTERVAL_MS * (2 ** Math.min(state.failures, 4)));
    setConnection("error", unauthorized ? "会话失效" : "连接异常"); showAlert(unauthorized ? "管理员会话已失效" : "实时数据暂时不可用", unauthorized ? "重新验证后可继续访问。" : error.message, unauthorized ? "重新登录" : "重试");
    ui.loading.classList.remove("is-visible"); ui.footerStatus.textContent = unauthorized ? "身份验证失败" : `将在 ${Math.ceil(backoff / 1000)} 秒后重试`; if (!unauthorized) scheduleNext(backoff);
  } finally { if (state.inFlight === controller) { state.inFlight = null; ui.refreshButton.disabled = false; ui.refreshButton.classList.remove("is-loading"); } }
}

async function reloadRuntime() {
  ui.reloadButton.disabled = true; setConnection("pending", "重载中");
  try { const control = await fetchJson("/ai-bridge/api/reload", { method: "POST", mutate: true }); renderControl(control, true); state.snapshot = null; hideAlert(true); await refreshAfterMutation(); }
  catch (error) { showAlert("重载失败", error.message, "重试", true); setConnection("error", "重载失败"); }
  finally { ui.reloadButton.disabled = false; }
}
async function saveConfig() {
  ui.configSave.disabled = true;
  try { const control = await fetchJson("/ai-bridge/api/config", { method: "PUT", mutate: true, body: JSON.stringify({ values: Object.fromEntries(state.configOverrides) }) }); renderControl(control, true); hideAlert(true); await refreshAfterMutation(); }
  catch (error) { showAlert("配置未应用", error.message, "重试", true); ui.configSave.disabled = false; }
}
function openListDialog(kind, mode, id = "", label = "", currentValue = "") {
  state.dialog = { kind, mode, id, label }; const keyList = kind !== "blacklist_ip";
  setText("dialog-title", mode === "create" ? `新增${keyList ? " Key" : " IP"}` : `修改 ${label}`); setText("list-value-label", keyList ? "完整 API Key" : "客户端 IP");
  ui.listValue.value = currentValue; ui.listValue.type = keyList ? "password" : "text"; ui.listValue.autocomplete = "off"; ui.listError.hidden = true; ui.listDialog.showModal(); ui.listValue.focus();
}
function closeListDialog() { ui.listValue.value = ""; if (ui.listDialog.open) ui.listDialog.close(); state.dialog = null; }
async function submitListDialog() {
  const value = ui.listValue.value.trim(); const { kind, mode, id } = state.dialog;
  try {
    const method = mode === "create" ? "POST" : "PUT"; const body = mode === "create" ? { value } : { id, value };
    await fetchJson(`/ai-bridge/api/lists/${kind}`, { method, mutate: true, body: JSON.stringify(body) }); closeListDialog(); await refreshAfterMutation(true);
  } catch (error) { ui.listError.textContent = error.message; ui.listError.hidden = false; }
}
async function deleteListEntry(button) {
  if (!window.confirm(`确认删除 ${button.dataset.label}？`)) return;
  try { await fetchJson(`/ai-bridge/api/lists/${button.dataset.deleteList}`, { method: "DELETE", mutate: true, body: JSON.stringify({ id: button.dataset.id }) }); await refreshAfterMutation(true); }
  catch (error) { showAlert("删除失败", error.message, "重试", true); }
}
async function clearCooldown(button) {
  if (!window.confirm(`确认解除 ${button.dataset.label} 的冷却？`)) return;
  button.disabled = true;
  try { await fetchJson(`/ai-bridge/api/cooldowns/${button.dataset.clearCooldown}`, { method: "DELETE", mutate: true, body: JSON.stringify({ id: button.dataset.id }) }); state.snapshot = null; await refreshAfterMutation(); }
  catch (error) { if (error.status === 404) { state.snapshot = null; await refreshAfterMutation(); } else { showAlert("解除冷却失败", error.message, "重试", true); button.disabled = false; } }
}

document.querySelectorAll("[role='tab']").forEach((tab) => tab.addEventListener("click", () => setView(tab.dataset.view)));
document.querySelectorAll("[data-open-view]").forEach((button) => button.addEventListener("click", () => setView(button.dataset.openView)));
document.addEventListener("click", (event) => {
  const create = event.target.closest("[data-create-list]"); if (create) openListDialog(create.dataset.createList, "create");
  const edit = event.target.closest("[data-edit-list]"); if (edit) openListDialog(edit.dataset.editList, "update", edit.dataset.id, edit.dataset.label, edit.dataset.currentValue || "");
  const remove = event.target.closest("[data-delete-list]"); if (remove) deleteListEntry(remove);
  const cooldown = event.target.closest("[data-clear-cooldown]"); if (cooldown) clearCooldown(cooldown);
});
ui.listForm.addEventListener("click", (event) => { if (event.target.closest("button[value='cancel']")) { event.preventDefault(); closeListDialog(); } });
ui.listForm.addEventListener("submit", (event) => { event.preventDefault(); if (event.submitter?.value === "cancel") { closeListDialog(); return; } submitListDialog(); });
ui.searchInput.addEventListener("input", () => { state.query = ui.searchInput.value.trim().toLocaleLowerCase(); render(); });
ui.refreshButton.addEventListener("click", () => refresh(true)); ui.reloadButton.addEventListener("click", reloadRuntime); ui.configSave.addEventListener("click", saveConfig);
ui.configResetAll.addEventListener("click", () => { state.configOverrides.clear(); setConfigDirty(true); renderConfigFields(); });
ui.alertAction.addEventListener("click", () => { if (ui.alertTitle.textContent.includes("会话")) window.location.assign("/ai-bridge"); else { state.alertSticky = false; hideAlert(true); refresh(true); } });
document.addEventListener("visibilitychange", () => { if (document.hidden) { window.clearTimeout(state.pollTimer); if (state.inFlight) state.inFlight.abort(); setConnection("pending", "已暂停"); } else refresh(false); });
window.setInterval(() => document.querySelectorAll("[data-expires-at]").forEach((node) => { node.textContent = formatRemaining(Number(node.dataset.expiresAt)); }), 1000);
setView("overview"); refresh(false);
