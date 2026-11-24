---
name: claude-grep
description: Search your past Claude Code conversations by what was said in them
argument-hint: "[all] <query>"
disable-model-invocation: true
---

# Search old Claude Code conversations

`/salash:claude-grep [all] <query>` lists past Claude Code sessions in which the user or
Claude said something matching the query, newest first, so the user can resume one. Only
message text counts: tool calls, tool output, thinking and JSON keys don't.

The argument is: `$ARGUMENTS`

## 1. Check the argument

If it is empty (or only `all` / `--all`), ask the user what to search for and stop.

## 2. Build the query

Never pass the user's text to a shell command as it is. Turn it into the query JSON below.

| `all` / `--all` as the first word | `"all": true`: every project. Otherwise just this project and its worktrees. |
|---|---|
| `a OR b`, `a \| b` | one term with two alternatives: `["a", "b"]` (either may appear) |
| `a AND b`, `a b` | two terms: `["a"], ["b"]` (both must appear in the same session) |
| `"exact phrase"` | one alternative with the whole phrase: `["exact phrase"]` |
| `-word`, `NOT word` | a term in `neg`: drop sessions that say it |
| capitals typed by the user | `"icase": false` (smart case). Otherwise `"icase": true`. |
| free text ("that time we fixed the resize bug") | pick the 1–3 key terms, add close synonyms and spellings as alternatives (`["resize", "resizing", "SIGWINCH"], ["bug"]`). Drop filler words. |

Every alternative is a literal string. Regex characters in it match themselves, so don't
escape them. Write the strings as valid JSON (escape `"` and `\`). `pos` must have at least
one term. If after this you can't tell what the user wants to find, ask.

Before the results, show the query in this form, so the user can see what was searched:
`(resize | resizing | SIGWINCH) AND bug, NOT tmux` (this project).

## 3. Run the search

Run this as a single Bash command. Replace only the line between `<<'QUERY'` and `QUERY`
with your query JSON. Keep everything else as it is.

```sh
d=$(mktemp -d "${TMPDIR:-/tmp}/claude-grep.XXXXXX")
cat > "$d/query.json" <<'QUERY'
{"all": false, "icase": true, "pos": [["resize", "resizing", "SIGWINCH"], ["bug"]], "neg": [["tmux"]]}
QUERY
cat > "$d/said.jq" <<'JQ'
# The text of one record if it is something the user typed or Claude wrote.
def said:
  select(.isMeta != true and .isCompactSummary != true)
  | if .type == "user" then
      .message.content
      | if type == "string" then .
        elif type == "array" and all(.[]; .type == "text") then [.[].text] | join(" ")
        else empty end
      | select(test("^\\s*<(command-|local-command|system-reminder|task-notification)") | not)
    elif .type == "assistant" then [.message.content[]? | select(.type == "text") | .text] | join(" ")
    else empty end
  | gsub("<system-reminder>[\\s\\S]*?</system-reminder>"; "") | gsub("\\s+"; " ") | select(test("\\S"));
def esc: gsub("(?<c>[.^$|?*+()\\[\\]{}\\\\])"; "\\\(.c)");
def short($n): if length > $n then .[:$n] + "…" else . end;
$Q[0] as $q
| (if $q.icase then "i" else "" end) as $fl
| ($q.pos | map(map(esc) | join("|"))) as $pos
| ($q.neg | map(map(esc) | join("|"))) as $neg
| [inputs | fromjson?] as $recs
| [$recs[] | said] as $msgs
| select(all($pos[]; . as $re | any($msgs[]; test($re; $fl))))
| select(any($neg[]; . as $re | any($msgs[]; test($re; $fl))) | not)
| ([$recs[] | .cwd? // empty] | first // "?" | split("/") | last) as $proj
| (([$recs[] | select(.type == "ai-title") | .aiTitle] | last) // ($msgs[0] // "") | short(70)) as $title
| ([$pos[] as $re | first($msgs[] | select(test($re; $fl)) | match($re; $fl) as $m
     | ([$m.offset - 50, 0] | max) as $s
     | (if $s > 0 then "…" else "" end) + .[$s:$m.offset + $m.length + 50]
     + (if $m.offset + $m.length + 50 < length then "…" else "" end))]
   | reduce .[] as $x ([]; if index([$x]) then . else . + [$x] end) | .[:2] | join(" | ")) as $snip
| [$proj, $title, $snip] | @tsv
JQ
root="${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects"
enc() { sed 's/[^A-Za-z0-9-]/-/g'; }  # a path, as Claude names its project folder
if [ "$(jq -r .all "$d/query.json")" = true ]; then
  find "$root" -mindepth 1 -maxdepth 1 -type d > "$d/dirs"
else
  top=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null) && top=$(dirname "$top") || top=$(pwd -P)
  { printf '%s\n' "$top"; git worktree list --porcelain 2>/dev/null | sed -n 's/^worktree //p'; } |
    while IFS= read -r p; do printf '%s/%s\n' "$root" "$(printf '%s' "$p" | enc)"; done > "$d/dirs"
  find "$root" -mindepth 1 -maxdepth 1 -type d -name "$(printf '%s' "$top" | enc)--claude-worktrees-*" >> "$d/dirs"
fi
# Quick pass on the raw files: sessions whose JSON has the first term anywhere.
jq -r '.pos[0][] | tojson | .[1:-1]' "$d/query.json" > "$d/pat"
ci=$(jq -r 'if .icase then "-i" else "--case-sensitive" end' "$d/query.json")
sort -u "$d/dirs" | while IFS= read -r p; do
  [ -d "$p" ] || continue
  if command -v rg >/dev/null 2>&1; then
    rg -l -F "$ci" --max-depth 1 -g '*.jsonl' -f "$d/pat" -- "$p"
  else
    find "$p" -mindepth 1 -maxdepth 1 -type f -name '*.jsonl' -exec grep -l -F $( [ "$ci" = -i ] && echo -i ) -f "$d/pat" {} +
  fi
done > "$d/cand"
# Full check on what was actually said, newest session first.
while IFS= read -r f; do
  [ "$(basename "$f" .jsonl)" = "${CLAUDE_SESSION_ID}" ] && continue
  hit=$(jq -nrR --slurpfile Q "$d/query.json" -f "$d/said.jq" "$f" 2>/dev/null) || continue
  [ -n "$hit" ] && printf '%s\t%s\t%s\t%s\n' "$(date -r "$f" +%s)" "$(date -r "$f" '+%Y-%m-%d %H:%M')" "$(basename "$f" .jsonl)" "$hit"
done < "$d/cand" | sort -rn | cut -f2- > "$d/hits"
echo "matches: $(wc -l < "$d/hits" | tr -d ' ')"
head -n 20 "$d/hits"
rm -rf "$d"
```

It prints `matches: N` and up to 20 lines, newest first, with tab-separated fields: date,
session id, project (the last part of the folder Claude ran in), title, and snippets. If `jq`
is missing, tell the user that claude-grep needs it and stop.

## 4. Report

- The query from step 2 and the scope (this project, or all projects).
- One line per session as printed: date, project, session id, title, then the snippet(s).
  Keep the full session id, since that's what resume needs. Shorten a snippet if it is very
  long, but keep the matching words.
- If `matches` is more than 20: "N more; narrow the query or add a term."
- If there are no matches, say so and suggest a broader query: `all`, an `OR` alternative, or
  fewer terms.
- End with how to resume one: `claude --resume <session id>` in the project's folder, or in
  salash press `I` and paste the session id to open it in a new tab.

Don't open, summarize or resume a session unless the user asks.
