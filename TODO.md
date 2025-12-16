TODO

Since the Copilot CLI is installed via global npm, your wrapper should:
• call command -v copilot at runtime
• if missing, print a friendly error telling them to install it (npm i -g ... or whatever the official install is)

Change the 'confirm execution' to be something you can just press y without enter.

• Add a copilot-wrapper doctor command that checks:
• copilot exists
• clipboard tools exist
• terminal is interactive
• prints fixes
