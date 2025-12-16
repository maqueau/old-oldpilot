# Copilot CLI “command mode” (prefix-gated)

IMPORTANT: Only apply the rules below when the user prompt begins with one of these exact tokens:

- [[CPLT:CS]]
- [[CPLT:CE]]
  If the prompt does NOT start with one of those tokens, IGNORE ALL RULES IN THIS FILE and respond normally.

---

## [[CPLT:CS]] — Suggest a terminal command (command-only)

When the prompt begins with [[CPLT:CS]]:

1. Return EXACTLY ONE (1) executable zsh command line that accomplishes the task.
2. Output ONLY the command text. No prose, no Markdown, no backticks, no code fences, no numbering, no extra whitespace.
3. The command must be something the user can paste/run directly in macOS zsh.
4. If multiple steps are required, combine them into a single line using operators like `&&` (still one line).
5. If the task is ambiguous, choose the most reasonable default WITHOUT asking follow-up questions.
6. Safety: Avoid destructive operations by default (e.g., rm, mv, chmod on broad globs). If the user explicitly requests a destructive action, prefer safer flags (interactive prompts, dry-run equivalents) when available.

Examples (for your behavior):

- “output hello world to terminal” -> `echo "hello world"`
- “find large files” -> `find . -type f -size +100M -print`

---

## [[CPLT:CE]] — Explain a terminal command (explain-only)

When the prompt begins with [[CPLT:CE]]:

1. Assume the remainder of the prompt contains a shell command to explain.
2. Provide a concise explanation of what the command does, including important flags and effects.
3. Do NOT suggest alternative commands unless asked.
4. If no command is present, respond with a single sentence telling the user to provide a command to explain.
