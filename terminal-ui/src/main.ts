import "@xterm/xterm/css/xterm.css";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { spawn } from "tauri-pty";

// ── Terminal Theme (matches our CSS palette) ──────────────────────────────────
const TERMINAL_THEME = {
  background: "#0d1117",
  foreground: "#e6edf3",
  cursor: "#38bdf8",
  cursorAccent: "#0d1117",
  selectionBackground: "rgba(56, 189, 248, 0.25)",
  selectionForeground: "#e6edf3",

  // ANSI Colors (carefully chosen for readability on dark bg)
  black: "#484f58",
  red: "#f87171",
  green: "#34d399",
  yellow: "#fb923c",
  blue: "#38bdf8",
  magenta: "#a78bfa",
  cyan: "#22d3ee",
  white: "#e6edf3",

  // Bright variants
  brightBlack: "#6e7681",
  brightRed: "#fca5a5",
  brightGreen: "#6ee7b7",
  brightYellow: "#fcd34d",
  brightBlue: "#7dd3fc",
  brightMagenta: "#c4b5fd",
  brightCyan: "#67e8f9",
  brightWhite: "#ffffff",
};

// ── Initialize Terminal ───────────────────────────────────────────────────────
const container = document.getElementById("terminal-container")!;
const statusDot = document.querySelector("#status-dot .dot") as HTMLElement;
const statusText = document.querySelector("#status-dot .status-text") as HTMLElement;

const term = new Terminal({
  fontFamily: "'JetBrains Mono', 'SF Mono', 'Cascadia Code', 'Fira Code', monospace",
  fontSize: 14,
  lineHeight: 1.35,
  letterSpacing: 0,
  cursorBlink: true,
  cursorStyle: "bar",
  cursorWidth: 2,
  theme: TERMINAL_THEME,
  allowProposedApi: true,
  scrollback: 10000,
  smoothScrollDuration: 100,
  macOptionIsMeta: true,
  macOptionClickForcesSelection: true,
});

// ── Addons ────────────────────────────────────────────────────────────────────
const fitAddon = new FitAddon();
term.loadAddon(fitAddon);
term.loadAddon(new WebLinksAddon());

// ── Mount & Fit ───────────────────────────────────────────────────────────────
term.open(container);
fitAddon.fit();

// ── Spawn PTY ─────────────────────────────────────────────────────────────────
function setConnected(connected: boolean) {
  if (connected) {
    statusDot.classList.remove("disconnected");
    statusText.textContent = "Connected";
  } else {
    statusDot.classList.add("disconnected");
    statusText.textContent = "Disconnected";
  }
}

try {
  // Determine the user's shell
  const shell = getShell();

  const pty = spawn(shell, [], {
    cols: term.cols,
    rows: term.rows,
  });

  setConnected(true);

  // PTY → xterm (PTY emits Uint8Array)
  const decoder = new TextDecoder();
  pty.onData((data: Uint8Array) => {
    term.write(decoder.decode(data));
  });

  // xterm → PTY
  term.onData((data: string) => {
    pty.write(data);
  });

  // Handle PTY exit
  pty.onExit(({ exitCode }: { exitCode: number }) => {
    setConnected(false);
    term.write(`\r\n\x1b[38;5;243m[Process exited with code ${exitCode}]\x1b[0m\r\n`);
  });

  // Resize PTY when terminal resizes
  term.onResize(({ cols, rows }: { cols: number; rows: number }) => {
    pty.resize(cols, rows);
  });

} catch (err) {
  setConnected(false);
  term.write(`\x1b[31mFailed to spawn shell: ${err}\x1b[0m\r\n`);
}

// ── Auto-resize on window resize ──────────────────────────────────────────────
let resizeTimer: ReturnType<typeof setTimeout>;
window.addEventListener("resize", () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => {
    fitAddon.fit();
  }, 50);
});

// ── Focus terminal on click ───────────────────────────────────────────────────
container.addEventListener("click", () => {
  term.focus();
});

// Auto-focus on load
term.focus();

// ── Helper ────────────────────────────────────────────────────────────────────
function getShell(): string {
  // On macOS, use zsh (default shell since Catalina)
  return "/bin/zsh";
}
