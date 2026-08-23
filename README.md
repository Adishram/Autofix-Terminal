# AutoFix Terminal

AutoFix Terminal is a developer tool that wraps your command execution in a pseudo-terminal. When a command fails, the application automatically captures the error, locates the failing source code, and uses a local Large Language Model (via Ollama) to attempt to fix the error for you.

## Features

* Seamless Terminal Experience: Built with Tauri and xterm.js, providing a familiar terminal interface that inherits your system environment and shell configurations.
* Automatic Error Detection: Parses standard error output to detect syntax and runtime errors in multiple languages (Python, Rust, C/C++, TypeScript, Javascript, Java, Go).
* Local LLM Integration: Uses litellm and Ollama to generate code fixes locally, keeping your code private and avoiding API costs.
* Infinite Auto-Fix Loop: The terminal will keep trying to fix the error in a loop until it succeeds. You can stop it manually at any time using Ctrl+C, which will instantly restore the original file from a backup.
* Live Status Indicator: The GUI displays the real-time connection status of your local LLM.

## Prerequisites

* macOS
* Ollama installed and running locally on port 11434.

## Installation

1. Download the latest release from the Releases page.
2. Install the macOS application via the provided DMG installer.
3. Make sure Ollama is running in the background.

## Usage

Run any command using the `autofix` CLI wrapper. For example:

`autofix python3 script.py`

If the script fails, AutoFix Terminal will intercept the failure and attempt to resolve it automatically.
