#!/usr/bin/env python3
"""
ai_brain.py — AI-powered code fixer (Local Ollama Edition).

Accepts a source file path and raw stderr output, sends both to a local
Qwen 3.5 9B model via Ollama (through litellm), and outputs a strict JSON
payload with the corrected code and a short explanation.

Usage:
    python3 ai_brain.py <file_path> <stderr_output>

Environment:
    LITELLM_MODEL   — model identifier (default: ollama_chat/qwen3.5:9b)
    OLLAMA_API_BASE — Ollama server URL (default: http://localhost:11434)
"""

import sys
import os
import json
from pathlib import Path

from litellm import completion


# ── Load .env file automatically ───────────────────────────────────────────────
def load_dotenv():
    """Load .env file from script directory or cwd."""
    candidates = [
        Path(__file__).resolve().parent / ".env",
        Path.cwd() / ".env",
    ]
    for env_path in candidates:
        if env_path.exists():
            with open(env_path, "r") as f:
                for line in f:
                    line = line.strip()
                    if not line or line.startswith("#"):
                        continue
                    if "=" in line:
                        key, _, value = line.partition("=")
                        key = key.strip()
                        value = value.strip()
                        # Don't overwrite existing env vars
                        if key not in os.environ:
                            os.environ[key] = value
            break

load_dotenv()


# ── Speed-optimized prompt for local 9B model ──────────────────────────────────
# Kept short and direct to minimize token processing time.
# No chain-of-thought, no preamble — just fix and return JSON.
SYSTEM_PROMPT = """\
You are a code fixer. Given source code and its error output, fix the bug.
Return ONLY valid JSON, nothing else:
{"fixed_code":"<entire corrected file>","explanation":"<2-3 sentence fix summary>"}
Rules:
- fixed_code must be the COMPLETE file, not a patch
- Preserve original intent and comments
- No markdown, no extra text, ONLY the JSON object"""


def main():
    if len(sys.argv) < 3:
        print(
            json.dumps({
                "fixed_code": "",
                "explanation": "Error: ai_brain.py requires two arguments: <file_path> <stderr_output>"
            }),
            file=sys.stdout,
        )
        sys.exit(1)

    file_path = sys.argv[1]
    stderr_output = sys.argv[2]

    # Read the source code
    try:
        with open(file_path, "r", encoding="utf-8") as f:
            source_code = f.read()
    except FileNotFoundError:
        print(
            json.dumps({
                "fixed_code": "",
                "explanation": f"Error: Could not read file '{file_path}'"
            }),
            file=sys.stdout,
        )
        sys.exit(1)

    # Compact user message — minimal tokens for speed
    user_message = f"FILE: {file_path}\nCODE:\n{source_code}\nERROR:\n{stderr_output}"

    # Model config — defaults to local Ollama Qwen 3.5 9B
    model = os.environ.get("LITELLM_MODEL", "ollama_chat/qwen3.5:9b")
    api_base = os.environ.get("OLLAMA_API_BASE", "http://localhost:11434")

    try:
        response = completion(
            model=model,
            messages=[
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": user_message},
            ],
            api_base=api_base,
            temperature=0.0,    # Deterministic — no creativity needed
            max_tokens=4096,    # Reduced for speed — fixes are typically small
            top_p=0.9,          # Slightly constrained for focused output
            num_retries=0,      # Fail fast — Rust handles retries
        )

        raw_content = response.choices[0].message.content.strip()

        # Strip markdown code fences if the model wraps them anyway
        if raw_content.startswith("```"):
            lines = raw_content.split("\n")
            if lines[-1].strip() == "```":
                lines = lines[1:-1]
            else:
                lines = lines[1:]
            raw_content = "\n".join(lines)

        # Handle case where model outputs ```json on first line
        if raw_content.startswith("json"):
            raw_content = raw_content[4:].strip()

        result = json.loads(raw_content)

        # Validate keys
        if "fixed_code" not in result or "explanation" not in result:
            raise ValueError("Missing required keys in LLM response")

        # Output strict JSON to stdout
        print(json.dumps(result), file=sys.stdout)

    except json.JSONDecodeError as e:
        print(
            json.dumps({
                "fixed_code": "",
                "explanation": f"Error: LLM returned invalid JSON — {e}"
            }),
            file=sys.stdout,
        )
        sys.exit(1)
    except Exception as e:
        print(
            json.dumps({
                "fixed_code": "",
                "explanation": f"Error calling LLM: {e}"
            }),
            file=sys.stdout,
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
