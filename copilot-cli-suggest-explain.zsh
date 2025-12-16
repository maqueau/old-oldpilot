# ---- Copilot CLI helpers -----------------------------------------------------

## ---- UI styling helpers -----------

# If terminal supports color, use it; otherwise print plain text.
__copilot_supports_color() {
  [[ -t 1 || -t 2 ]] && [[ "${TERM:-}" != "dumb" ]]
}

# Safe ANSI color wrappers (use via: $__C_BOLD, $__C_DIM, etc.)
__copilot_colors_init() {
  if [[ -t 1 || -t 2 ]] && [[ "${TERM:-}" != "dumb" ]]; then
    __C_RESET=$'\e[0m'
    __C_BOLD=$'\e[1m'
    __C_DIM=$'\e[2m'
    __C_CYAN=$'\e[36m'
    __C_GREEN=$'\e[32m'
    __C_YELLOW=$'\e[33m'
    __C_RED=$'\e[31m'
    __C_GRAY=$'\e[90m'
  else
    __C_RESET=""
    __C_BOLD=""
    __C_DIM=""
    __C_CYAN=""
    __C_GREEN=""
    __C_YELLOW=""
    __C_RED=""
    __C_GRAY=""
  fi
}

__copilot_colors_init

# Print a label + indented body with tight spacing
__copilot_print_block() {
  local label="$1"
  local body="$2"

  # indent every line of body by 2 spaces
  local indented
  indented="$(printf "%s" "$body" | sed 's/^/  /')"

  # tight: no leading blank line; one trailing newline
  printf "%s%s%s\n%s\n" "${__C_BOLD}${__C_CYAN}" "$label" "${__C_RESET}" "$indented" >/dev/tty
}

## -----------------------------------

# Default model (non-premium in your table)
export COPILOT_DEFAULT_MODEL="gpt-4.1"

# Rare prefixes so instructions never trigger accidentally
export COPILOT_CS_PREFIX="[[CPLT:CS]]"
export COPILOT_CE_PREFIX="[[CPLT:CE]]"

# require [y/N] before execute; set to false later if you want
export COPILOT_CS_CONFIRM_EXECUTE="${COPILOT_CS_CONFIRM_EXECUTE:-true}"

__clip_copy() {
  if command -v pbcopy >/dev/null 2>&1; then
    pbcopy
  elif command -v wl-copy >/dev/null 2>&1; then
    wl-copy
  elif command -v xclip >/dev/null 2>&1; then
    xclip -selection clipboard
  else
    return 1
  fi
}

__copilot_prompt() {
  local prompt_text="$1"
  local model="$2"

  if [[ -n "$model" ]]; then
    copilot -p "$prompt_text" -s --model "$model"
  else
    copilot -p "$prompt_text" -s
  fi
}

ce()  { __copilot_prompt "${COPILOT_CE_PREFIX} $*" "$COPILOT_DEFAULT_MODEL"; }
cex() { __copilot_prompt "${COPILOT_CE_PREFIX} $*" ""; }

__copilot_action_picker() {
  local options labels hotkeys
  options=("execute" "copy" "explain" "quit")
  labels=("Execute command" "Copy to clipboard" "Explain w/ Copilot" "Quit")
  hotkeys=("↵" "c" "x" "q")

  local idx=1
  local key seq1 seq2
  local lines=$((1 + ${#options[@]})) # header + options

  __draw() {
    printf "%sAction (↑/↓ + Enter, or hotkeys):%s\n" "${__C_DIM}" "${__C_RESET}" >/dev/tty

    local i
    for i in {1..${#options[@]}}; do
      if [[ $i -eq $idx ]]; then
        printf "  %s> %s%s  %s(%s)%s\n" \
          "${__C_BOLD}${__C_GREEN}" \
          "${labels[$i]}" \
          "${__C_RESET}" \
          "${__C_GRAY}" \
          "${hotkeys[$i]}" \
          "${__C_RESET}" >/dev/tty
      else
        printf "    %s  %s(%s)%s\n" \
          "${labels[$i]}" \
          "${__C_GRAY}" \
          "${hotkeys[$i]}" \
          "${__C_RESET}" >/dev/tty
      fi
    done
  }

  __clear() {
    local n
    for n in {1..$lines}; do
      printf "\033[1A\033[2K\r" >/dev/tty
    done
  }

  __draw

  while true; do
    # -s = silent (no echo), -k 1 = one char
    IFS= read -rsk 1 key </dev/tty

    # Hotkeys
    case "$key" in
      c|C) __clear; echo "copy"; return ;;
      x|X) __clear; echo "explain"; return ;;
      q|Q) __clear; echo "quit"; return ;;
      ""|$'\n'|$'\r')  __clear; echo "${options[$idx]}"; return ;; # Enter
    esac

    # Arrow keys: ESC [ A/B
    if [[ "$key" == $'\e' ]]; then
      IFS= read -rsk 1 seq1 </dev/tty
      [[ "$seq1" == "[" ]] || continue
      IFS= read -rsk 1 seq2 </dev/tty

      case "$seq2" in
        A) # Up
          (( idx = idx - 1 ))
          (( idx < 1 )) && idx=${#options[@]}
          __clear; __draw
          ;;
        B) # Down
          (( idx = idx + 1 ))
          (( idx > ${#options[@]} )) && idx=1
          __clear; __draw
          ;;
      esac
    fi
  done
}

__copilot_confirm_execute() {
  local cmd="$1"
  if [[ "$COPILOT_CS_CONFIRM_EXECUTE" != "true" ]]; then
    return 0
  fi

  # printf "\nAbout to execute:\n  %s\nProceed? [y/N] " "$cmd" >/dev/tty
  printf "Confirm execution: [y/N] " "$cmd" >/dev/tty
  local ans
  IFS= read -r ans </dev/tty
  [[ "$ans" == "y" || "$ans" == "Y" ]]
}

__copilot_suggest_then_act() {
  local prompt_text="$1"
  local model="$2"
  local suggestion trimmed action

  suggestion="$(__copilot_prompt "${COPILOT_CS_PREFIX} TASK: $prompt_text" "$model")" || {
    # If Copilot errors, print whatever it printed and stop.
    return $?
  }

  trimmed="$(echo "$suggestion" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"

  if [[ -z "$trimmed" ]]; then
    printf "No suggestion returned.\n" >/dev/tty
    return 1
  fi

  __copilot_print_block "Copilot" "$trimmed"

  action="$(__copilot_action_picker)"
  case "$action" in
    execute)
      if __copilot_confirm_execute "$trimmed"; then
        eval "$trimmed"
      else
        printf "%s%s%s\n" "${__C_YELLOW}" "Canceled." "${__C_RESET}" >/dev/tty
      fi
      ;;
    copy)
      if echo -n "$trimmed" | __clip_copy; then
        printf "%s%s%s\n" "${__C_GREEN}" "Copied to clipboard." "${__C_RESET}" >/dev/tty
      else
        printf "No clipboard tool found (pbcopy/wl-copy/xclip).\n" >/dev/tty
        return 1
      fi
      ;;
    explain)
      ce "Explain this command: $trimmed"
      ;;
    quit)
      printf "%s%s%s\n" "${__C_YELLOW}" "Canceled." "${__C_RESET}" >/dev/tty
      ;;
  esac
}

cs()  { __copilot_suggest_then_act "$*" "$COPILOT_DEFAULT_MODEL"; }
csx() { __copilot_suggest_then_act "$*" ""; }

# -----------------------------------------------------------------------------