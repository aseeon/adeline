# UI performance

Adeline uses isolated chat views, virtual lists, prepared content, background search/parsing, and independent file/document/service views.

## Update boundaries

The header, chat sidebar, transcript and composer are separate GPUI entities. The header, sidebar and transcript use explicit `AnyView::cached` boundaries. The small composer keeps its intrinsic layout and updates independently. Text inputs emit a content-change event; caret movement and selection no longer trigger the shell's search observer.

`Adeline` still owns the shared data and coordinates actions. Child views hold weak references to it and own their subscriptions and list state. `sync_regions` explicitly notifies views affected by each action. New data-changing actions must update this dependency map to prevent stale cached content. Resize and native window refresh can invalidate caches through GPUI.

## Virtual lists

Chat cards and messages use GPUI's variable-height `list` with persistent `ListState`. Only visible rows and a small overscan area are constructed. Filtering retains matching indices and stable chat IDs. Selecting a chat preserves the sidebar scroll anchor; switching project or changing the visible result set resets it. Sending scrolls to the newly appended messages.

Changed rows invalidate cached measurements while retaining their logical scroll anchor. GPUI remeasures rows when viewport width changes. Bundled message images reserve their aspect ratio before loading. Decision footers participate in the list's focus tracking.

Search text is normalized when chat content changes. Status counting no longer joins or lowercases message text. Search requests reuse immutable index snapshots; changing the query does not copy chat messages. Full-text semantics, including matches spanning message boundaries and Unicode case conversion, are preserved.

## Prepared content and background work

Documents retain parsed blocks, original source-line indices, raw text, table cells, preview blocks and word counts. File titles and service names retain normalized search text. Service output is split into shared lines once. Painting and scrolling reuse these results. Edits invalidate document revisions; title changes update title search separately.

Chat searches use a background executor at 256 chats or 64 KiB of indexed text. A cancellable 30 ms delay combines rapid keystrokes before scanning. Each request has a generation ticket, so an older worker cannot overwrite a newer query or project. Small searches run synchronously.

Documents of 32 KiB or more are parsed on a background executor, both on initial preparation and after edits. Workers receive shared immutable source snapshots. Results publish only to the matching project/document revision. While a changed document is being prepared, the view shows a loading label rather than editable rows with stale source-line positions. Small documents remain synchronous.

The extra normalized strings and parsed blocks trade some memory for less repeated CPU work. Search-index preparation and source-edit operations remain synchronous; background parsing does not make the entire editing pipeline asynchronous.

## File and service views

Files home, document content, service sidebar and service output have separate cached entities. The lightweight Files and Services wrappers remain uncached. This matters in GPUI 0.2.2: rebuilding a cached parent forces nested cached children to refresh. Keeping the heavy regions as siblings allows service search to update without rebuilding its log.

Documents and logs use variable-height virtual lists with persistent scroll state. Documents retain source positions when switching raw/rich views and preserve checkbox focus through list focus tracking. Row measurement uses an explicit capped document width to avoid GPUI 0.2.2 underestimating wrapped paragraph heights with nested percentage widths. Logs support wrapping, horizontal scrolling when unwrapped, and returning to the latest output when Follow is enabled.

Menus, hover or native window events can still cause framework-wide refreshes. Virtualization limits those refreshes to visible rows plus overscan. File-card grids and the small explorer/service inventories are not virtualized.

## Incremental updates

The next three recommendations were implemented in order:

1. Chat selection/status/send actions update the affected stable-ID search record. Unchanged records reuse their current membership; worker snapshots share groups of 64 records. Badge counts and unread-message indexes are maintained at mutation time. Explicit actions skip the 30 ms typing debounce, including empty-query filters. A pending search restarts against a fresh snapshot after a relevant mutation and still checks its generation before publishing. Project changes and inserted chats retain the full rebuild path.
2. Sidebar membership changes preserve matching prefixes/suffixes. A transcript append replaces its appended tail and decision footer. Documents retain unchanged row measurements and checkbox handles; a single-block edit invalidates one body row. Log Follow scrolls existing state, Stop updates the trailing status row, and Wrap invalidates heights. GPUI also invalidates heights on list width changes. Bulk document edits compare unchanged text around the edited range and preserve unaffected rows.
3. `Thread::push_message` extends a normalized, shared search index. Large existing text chunks are retained; small appended chunks are coalesced up to 16 KB. Searches support phrases spanning chunks and Unicode character boundaries. `Document::replace_line` edits a byte range without allocating every source line, updates one parsed block and the small preview, and adjusts the word count. Shared block storage copies at most 64 blocks and its chunk directory. Structural edits keep the full background-parser fallback.

The last valid document preview stays visible during a pending parse. Stale line-edit and checkbox actions are rejected until the new revision arrives. The same document's preview is retained; switching documents does not display another document's content.

Checkbox rows track their retained focus handle directly and register it as a tab stop. Button activation uses GPUI's built-in keyboard click handling; the duplicate key-down handler was removed because Space and Enter previously activated a retained button twice. Native checks confirmed click → Space toggles once, Tab advances to the next checkbox, and Enter activates it without moving the document.

## Running the profiling build

Normal builds omit the counters, file writes and synthetic data. Close Adeline before rebuilding on Windows.

```powershell
cargo run --release --locked --features ui-profiling
```

The profiling build writes cumulative counters every 250 ms to `artifacts/ui-profile.csv` under the source checkout. Run only one profiling instance at a time. For the large fixture, create the control file before launch:

```powershell
New-Item -ItemType Directory -Path artifacts -Force | Out-Null
Set-Content artifacts/ui-stress-rows.txt 10000
```

Open a chat, focus the composer, and let initial rendering settle. Save a snapshot, type or move the caret without changing window size or focus, then wait at least 250 ms and save another snapshot:

```powershell
Import-Csv artifacts/ui-profile.csv | Select-Object -Last 1 | Export-Csv artifacts/before.csv -NoTypeInformation
# Perform the input gesture, then capture the second snapshot.
Import-Csv artifacts/ui-profile.csv | Select-Object -Last 1 | Export-Csv artifacts/after.csv -NoTypeInformation
./scripts/check-ui-isolation.ps1 -Before artifacts/before.csv -After artifacts/after.csv
```

Set `artifacts/ui-stress-rows.txt` to `0` to restore normal fixtures in later profiling runs. `./scripts/package.ps1` always builds the normal release executable without enabling profiling.

Set `artifacts/ui-stress-content-rows.txt` to `10000` before launching a profiling build to add the large document and log. Set it to `0` afterwards. For document-dialog typing or service-search typing, take snapshots as above and run:

```powershell
./scripts/check-content-isolation.ps1 -Before artifacts/before.csv -After artifacts/after.csv -Region document
# Use -Region log when checking service search beside an open log.
```
