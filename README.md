# Autofix Terminal

Autofix Terminal is an auto-fixing developer environment and CLI tool. Run any command, and if it fails, a local AI will automatically attempt to fix the broken source code.

## Features

* Automatic Code Fixing: Captures standard error output from failed commands and automatically sends the broken code snippet and the error to a local LLM for correction.
* Infinite Retry Loop: The system will repeatedly attempt to fix and re-run your code until it succeeds or until you manually abort the process by pressing Ctrl-C.
* Safe Backups: Automatically creates a backup of the original file before making any changes. If you abort the fix process, the original file is safely restored.
* Local First: Built to integrate directly with Ollama. Your code never leaves your machine unless you explicitly configure an external provider.
* Standalone Terminal GUI: Includes a modern, hardware-accelerated terminal emulator built with Tauri and xterm.js.
* Connection Monitoring: Automatically monitors and displays your connection status to the local LLM endpoint.

## Prerequisites

* Rust and Cargo
* Node.js and pnpm
* Ollama running locally (for the default local LLM experience)

## Installation

You can download the latest macOS installer from the Releases page, or build it from source:

1. Clone the repository
2. Install the CLI tool globally:
   cargo install --path .
3. Build the Terminal GUI:
   cd terminal-ui
   pnpm install
   pnpm run tauri build

## Usage

You can use the Autofix Terminal GUI directly. Alternatively, you can use the command line tool from any terminal:

autofix -- <your_command>

Example:
autofix -- python3 script.py

If the script fails, autofix will locate the error, send it to the AI, apply the fix, and automatically re-run the script.

## Configuration

By default, the tool expects Ollama to be running locally on http://localhost:11434 with the qwen3.5:9b model. You can override this by configuring the following environment variables:

* LITELLM_MODEL: The model identifier to use.
* OLLAMA_API_BASE: The API endpoint for your LLM provider.
