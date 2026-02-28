import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import "./App.css";
import logoWhite from "./assets/logo-white.png";
import logoBlack from "./assets/logo-black.png";

/* ── Types ── */
interface DailyActivity { date: string; messageCount: number; sessionCount: number; toolCallCount: number }
interface DailyModelTokens { date: string; tokensByModel: Record<string, number> }
interface ModelUsage { inputTokens: number; outputTokens: number; cacheReadInputTokens?: number; cacheCreationInputTokens?: number }
interface LongestSession { duration: number; messageCount: number }
interface Stats {
  dailyActivity: DailyActivity[]; dailyModelTokens: DailyModelTokens[];
  modelUsage: Record<string, ModelUsage>; totalSessions: number; totalMessages: number;
  firstSessionDate?: string; longestSession?: LongestSession;
}
interface TokenUsage { input: number; output: number; cacheRead: number; cacheCreation: number }
interface RealtimeStats {
  lastActivity: string | null;
  todayMessages: number;
  todayTokens: TokenUsage;
  weekMessages: number;
  weekTokens: TokenUsage;
  activeSessions: number;
  planType: string;
  rateLimitTier: string;
  todayModelTokens: Record<string, number>;
  weekModelTokens: Record<string, number>;
}
interface UsageClaim {
  utilization: number;   // 0.0 - 1.0
  reset: number | null;  // unix timestamp
  status: string;
}
interface RateLimitInfo {
  status: string;
  representativeClaim: string | null;
  fiveHour: UsageClaim | null;
  sevenDay: UsageClaim | null;
  sevenDaySonnet: UsageClaim | null;
  overageStatus: string | null;
  overageDisabledReason: string | null;
  overageReset: number | null;
  fallbackPercentage: number | null;
  checkedAt: string;
}

/* ── Helpers ── */
const WINDOW_MS = 5 * 36e5;
const f = (n: number) => n >= 1e9 ? (n/1e9).toFixed(1)+"B" : n >= 1e6 ? (n/1e6).toFixed(1)+"M" : n >= 1e3 ? (n/1e3).toFixed(1)+"K" : n.toLocaleString();

async function notify(title: string, body: string) {
  let ok = await isPermissionGranted();
  if (!ok) { const perm = await requestPermission(); ok = perm === "granted"; }
  if (ok) sendNotification({ title, body });
}

function fmtClock(d: Date) {
  return d.toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
}

function fmtAgo(d: Date | null, now: Date) {
  if (!d) return "";
  const sec = Math.floor((now.getTime() - d.getTime()) / 1000);
  if (sec < 5) return "just now";
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  return `${Math.floor(hr / 24)}d ago`;
}

function planLabel(p: string) {
  if (p === "max") return "Max";
  if (p === "pro") return "Pro";
  if (p === "team") return "Team";
  return p.charAt(0).toUpperCase() + p.slice(1);
}

function fmtResetCountdown(resetUnix: number | null | undefined, now: Date) {
  if (!resetUnix) return "";
  const diff = resetUnix * 1000 - now.getTime();
  if (diff <= 0) return "now";
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ${min % 60}m`;
  return `${Math.floor(hr / 24)}d ${hr % 24}h`;
}

function fmtOverageReason(reason: string | null | undefined): string {
  if (!reason) return "Not available";
  if (reason.includes("org_level")) return "Disabled by org admin";
  if (reason.includes("not_enabled")) return "Not enabled on your plan";
  if (reason.includes("billing")) return "Billing setup required";
  if (reason.includes("limit")) return "Overage limit reached";
  if (reason.includes("admin")) return "Disabled by admin";
  return reason.replace(/_/g, " ").replace(/^\w/, c => c.toUpperCase());
}

function claimPct(c: UsageClaim | null) {
  if (!c) return 0;
  return Math.round(c.utilization * 100);
}

/* ── Detect window type ── */
const IS_POPOVER = getCurrentWebviewWindow().label === "popover";

/* ── App ── */
export default function App() {
  return IS_POPOVER ? <Popover /> : <Dashboard />;
}

/* ═══════════════════════════════════════════
   Popover — shown on tray click
   ═══════════════════════════════════════════ */
function Popover() {
  const [rl, setRl] = useState<RateLimitInfo | null>(null);
  const [clock, setClock] = useState(() => new Date());

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
  }, []);

  const load = useCallback(async () => {
    try {
      const data = await invoke<RateLimitInfo>("get_rate_limits", { force: false });
      setRl(data);
    } catch {}
  }, []);

  useEffect(() => {
    load();
    const a = setInterval(load, 10000);
    const c = setInterval(() => setClock(new Date()), 1000);
    let u: (() => void) | undefined;
    listen("claude-data-changed", () => load()).then(fn => { u = fn; });
    return () => { clearInterval(a); clearInterval(c); u?.(); };
  }, [load]);

  return (
    <div className="popover">
      {rl?.fiveHour && (() => {
        const pct = claimPct(rl.fiveHour);
        return (
          <div className="pop-section pop-hero">
            <div className="section-title">
              Current Session
              <span className="section-tag">LIVE</span>
            </div>
            <div className="limit-card">
              <div className="limit-header">
                <div>
                  <div className="limit-sub hero-reset">{pct >= 100 ? "Rate Limited" : `resets in ${fmtResetCountdown(rl.fiveHour!.reset, clock)}`}</div>
                </div>
                <div className={`limit-pct hero-pct ${pct >= 100 ? "done" : pct >= 80 ? "warn" : ""}`}>{pct}%</div>
              </div>
              <div className="limit-bar-wrap hero-bar">
                <div className={`limit-bar-fill ${pct >= 100 ? "done" : pct >= 80 ? "warn" : ""}`} style={{ width: `${Math.min(100, pct)}%` }} />
              </div>
            </div>
          </div>
        );
      })()}
    </div>
  );
}

/* ═══════════════════════════════════════════
   Dashboard — full main window
   ═══════════════════════════════════════════ */
function Dashboard() {
  const [stats, setStats] = useState<Stats | null>(null);
  const [rt, setRt] = useState<RealtimeStats | null>(null);
  const [rl, setRl] = useState<RateLimitInfo | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [tickCount, tick] = useState(0);
  const [clock, setClock] = useState(() => new Date());
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);
  const notifiedReset = useRef(false);
  const notifiedHigh = useRef(false);

  const loadStats = useCallback(async () => {
    try {
      const [a, b] = await Promise.all([
        invoke<Stats>("get_stats_cache"),
        invoke<RealtimeStats>("get_realtime_stats"),
      ]);
      setStats(a); setRt(b); setErr(null);
      setLastUpdated(new Date());
    } catch (e) { setErr(String(e)); }
  }, []);

  const loadRateLimits = useCallback(async (force = false) => {
    try {
      const data = await invoke<RateLimitInfo>("get_rate_limits", { force });
      setRl(data);
    } catch {}
    finally { setLoading(false); }
  }, []);

  const updateTray = useCallback((title: string) => {
    invoke("update_tray_title", { title }).catch(() => {});
  }, []);

  useEffect(() => {
    loadRateLimits();            // fast: show UI immediately after API call
    loadStats();                 // heavy: 698 JSONL files, runs in background
    const a = setInterval(loadStats, 30000);
    const rlInterval = setInterval(() => loadRateLimits(), 60000);
    const b = setInterval(() => tick(t => t + 1), 5000);
    const c = setInterval(() => setClock(new Date()), 1000);
    let u: (() => void) | undefined;
    listen("claude-data-changed", () => { loadStats(); loadRateLimits(); }).then(fn => { u = fn; });
    return () => { clearInterval(a); clearInterval(rlInterval); clearInterval(b); clearInterval(c); u?.(); };
  }, [loadStats, loadRateLimits]);

  useEffect(() => {
    // Tray: show 5h plan usage % if available
    if (rl?.fiveHour) {
      const rlPct = claimPct(rl.fiveHour);
      updateTray(rlPct >= 100 ? "FULL" : `${rlPct}%`);
    } else if (!rt?.lastActivity) {
      updateTray("REST");
    } else {
      const elapsed = Date.now() - new Date(rt.lastActivity).getTime();
      const pct = Math.min(100, (elapsed / WINDOW_MS) * 100);
      updateTray(pct >= 100 ? "REST" : `${Math.round(pct)}%`);
    }

    // Notifications
    if (rl?.fiveHour) {
      const pct5h = claimPct(rl.fiveHour);
      if (pct5h >= 100 && !notifiedReset.current) {
        notifiedReset.current = true;
        notify("Rate Limited", "5h session limit reached. Take a break.");
      }
      if (pct5h < 100) notifiedReset.current = false;
      if (pct5h >= 80 && !notifiedHigh.current) {
        notifiedHigh.current = true;
        notify("Usage Warning", `5h session at ${pct5h}%`);
      }
      if (pct5h < 70) notifiedHigh.current = false;
    }
  }, [rt, rl, updateTray, tickCount]);

  if (loading) return (
    <div className="app">
      <div className="drag-bar"/>
      <div className="center">
        <div className="ld-text">SPRT</div>
        <div className="ld-sub">Loading...</div>
      </div>
    </div>
  );

  if (err || !stats) {
    const isNoCreds = err?.includes("credentials");
    const isNoData = !err || err.includes("stats-cache");
    return (
      <div className="app">
        <div className="drag-bar"/>
        <Header planType={rt?.planType} lastUpdated={lastUpdated} clock={clock} />
        <div className="scroll">
          <div className="onboard">
            <div className="onboard-icon">{isNoCreds ? "🔑" : "📊"}</div>
            <div className="onboard-title">
              {isNoCreds ? "Login Required" : "No Data Yet"}
            </div>
            <div className="onboard-desc">
              {isNoCreds
                ? "Claude Code CLI is not logged in. Please authenticate first."
                : isNoData
                ? "No session data found. Start using Claude Code to see your stats here."
                : err}
            </div>
            <div className="onboard-steps">
              {isNoCreds ? (
                <>
                  <div className="onboard-step">1. Open Terminal</div>
                  <div className="onboard-step">2. Run <code>claude</code></div>
                  <div className="onboard-step">3. Follow the login prompts</div>
                </>
              ) : isNoData ? (
                <>
                  <div className="onboard-step">1. Install Claude Code: <code>npm i -g @anthropic-ai/claude-code</code></div>
                  <div className="onboard-step">2. Run <code>claude</code> in any project</div>
                  <div className="onboard-step">3. Stats will appear automatically</div>
                </>
              ) : null}
            </div>
          </div>
        </div>
      </div>
    );
  }

  const today = new Date().toISOString().split("T")[0];

  // Merge stats-cache daily data with today's realtime JSONL data
  const recent7Raw = stats.dailyActivity.slice(-7);
  const recent7 = recent7Raw.map(d => {
    if (d.date === today && rt) {
      return { ...d, messageCount: Math.max(d.messageCount, rt.todayMessages) };
    }
    return d;
  });
  const hasToday = recent7.some(d => d.date === today);
  if (!hasToday && rt && rt.todayMessages > 0) {
    recent7.push({ date: today, messageCount: rt.todayMessages, sessionCount: rt.activeSessions, toolCallCount: 0 });
    if (recent7.length > 7) recent7.shift();
  }

  const weekMsgs = rt?.weekMessages ?? recent7.reduce((s, d) => s + d.messageCount, 0);
  const maxM = Math.max(...recent7.map(d => d.messageCount), 1);
  const dayNames = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

  return (
    <div className="app">
      <div className="drag-bar" />
      <Header planType={rt?.planType} lastUpdated={lastUpdated} clock={clock} />

      <div className="scroll">
        {/* ── Current Session (5h) ── */}
        {rl?.fiveHour && (() => {
          const pct = claimPct(rl.fiveHour);
          return (
            <div className="glass-section session-hero">
              <div className="section-title">
                Current Session
                <span className="section-tag">LIVE</span>
              </div>
              <div className="limit-card">
                <div className="limit-header">
                  <div>
                    <div className="limit-sub hero-reset">{pct >= 100 ? "Rate Limited" : `resets in ${fmtResetCountdown(rl.fiveHour!.reset, clock)}`}</div>
                  </div>
                  <div className={`limit-pct hero-pct ${pct >= 100 ? "done" : pct >= 80 ? "warn" : ""}`}>{pct}%</div>
                </div>
                <div className="limit-bar-wrap hero-bar">
                  <div className={`limit-bar-fill ${pct >= 100 ? "done" : pct >= 80 ? "warn" : ""}`} style={{ width: `${Math.min(100, pct)}%` }} />
                </div>
              </div>
            </div>
          );
        })()}

        {/* ── Weekly Limits ── */}
        {rl && (rl.sevenDay || rl.sevenDaySonnet || rl.overageStatus) && (
          <div className="glass-section">
            <div className="section-title">Weekly Limits</div>
            {rl.sevenDay && (() => {
              const pct = claimPct(rl.sevenDay);
              return (
                <div className="limit-card">
                  <div className="limit-header">
                    <div>
                      <div className="limit-name">All Models</div>
                      <div className="limit-sub">resets in {fmtResetCountdown(rl.sevenDay!.reset, clock)}</div>
                    </div>
                    <div className={`limit-pct ${pct >= 100 ? "done" : pct >= 80 ? "warn" : ""}`}>{pct}%</div>
                  </div>
                  <div className="limit-bar-wrap">
                    <div className={`limit-bar-fill ${pct >= 100 ? "done" : pct >= 80 ? "warn" : ""}`} style={{ width: `${Math.min(100, pct)}%` }} />
                  </div>
                </div>
              );
            })()}
            {rl.sevenDaySonnet && (() => {
              const pct = claimPct(rl.sevenDaySonnet);
              return (
                <div className="limit-card">
                  <div className="limit-header">
                    <div>
                      <div className="limit-name">Sonnet</div>
                      <div className="limit-sub">resets in {fmtResetCountdown(rl.sevenDaySonnet!.reset, clock)}</div>
                    </div>
                    <div className={`limit-pct ${pct >= 100 ? "done" : pct >= 80 ? "warn" : ""}`}>{pct}%</div>
                  </div>
                  <div className="limit-bar-wrap">
                    <div className={`limit-bar-fill ${pct >= 100 ? "done" : pct >= 80 ? "warn" : ""}`} style={{ width: `${Math.min(100, pct)}%` }} />
                  </div>
                </div>
              );
            })()}
            {rl.overageStatus && (
              <div className="limit-card">
                <div className="limit-header">
                  <div>
                    <div className="limit-name">Extra Usage</div>
                    <div className="limit-sub">
                      {rl.overageStatus === "rejected"
                        ? fmtOverageReason(rl.overageDisabledReason)
                        : rl.overageStatus === "allowed" ? "Available when limits are reached" : "Active"}
                    </div>
                  </div>
                  <div className={`limit-pct ${rl.overageStatus === "rejected" ? "done" : ""}`}>
                    {rl.overageStatus === "rejected" ? "OFF" : "ON"}
                  </div>
                </div>
              </div>
            )}
            <div className="estimate-label">
              via OAuth API · checked {fmtAgo(new Date(rl.checkedAt), clock)}
            </div>
          </div>
        )}

        {/* ── 7-Day Chart ── */}
        <div className="glass-section">
          <div className="card-label">Last 7 Days · {f(weekMsgs)} messages</div>
          <div className="bars">
            {recent7.map(d => {
              const h = Math.max((d.messageCount / maxM) * 100, 4);
              const isNow = d.date === today;
              const dow = dayNames[new Date(d.date + "T00:00:00").getDay()];
              return (
                <div className="bar-col" key={d.date} title={`${d.date}: ${d.messageCount.toLocaleString()}`}>
                  <div className="bar-count">{d.messageCount > 0 ? f(d.messageCount) : ""}</div>
                  <div className={`bar ${isNow ? "today" : ""}`} style={{ height: `${h}%` }} />
                  <span className="bar-label">{isNow ? "Today" : dow}</span>
                </div>
              );
            })}
          </div>
        </div>
      </div>

      <div className="footer">
        <span>SPRT v0.2</span>
        <span>{fmtClock(clock)}</span>
      </div>
    </div>
  );
}

function Header({ planType, lastUpdated, clock }: { planType?: string; lastUpdated: Date | null; clock: Date }) {
  return (
    <div className="header">
      <div className="h-left">
        <img src={logoBlack} alt="SPRT" className="h-logo logo-light" />
        <img src={logoWhite} alt="SPRT" className="h-logo logo-dark" />
        <div className="h-titles">
          <div className="h-name">
            Claude Session Monitor
            {planType && planType !== "unknown" && <span className="plan-badge">{planLabel(planType)}</span>}
          </div>
          <div className="h-sub">
            {lastUpdated && <span>Updated {fmtAgo(lastUpdated, clock)}</span>}
          </div>
        </div>
      </div>
    </div>
  );
}
