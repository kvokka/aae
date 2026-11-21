# Agent Mail MCP tools

Agent Mail 0.3.40 returned these 45 names from MCP `tools/list` with filtering disabled. The addon defaults to 12
tools for its session, mail and file-reservation protocol. The other 33 remain available through Agent Mail's
filter settings. With `multi_repo = true` the default filter includes the five product tools (17 total). To list all tools, set `TOOLS_FILTER_ENABLED=false` in Agent Mail's `config.env` and start a new
service instance. The server reads its settings at startup.

## Enabled by default

| Purpose | Tools |
|---|---|
| Establish a session and find peers | `macro_start_session`, `list_agents` |
| Send, reply, receive and acknowledge | `send_message`, `reply_message`, `fetch_inbox`, `acknowledge_message` |
| Request and approve contact | `request_contact`, `respond_contact` |
| Reserve paths and check commits | `file_reservation_paths`, `renew_file_reservations`, `release_file_reservations`, `check_file_reservation_conflicts` |

## Available with a different filter

| Purpose | Tools | Why the default protocol omits them |
|---|---|---|
| Service health | `health_check` | LM probes the HTTP readiness endpoint. |
| Project and agent registration | `ensure_project`, `register_agent`, `create_agent_identity` | The session hook uses `macro_start_session` to establish its project and identity. |
| Identity administration | `retire_agent`, `unretire_agent`, `deregister_agent`, `whois` | Discovery with `list_agents` includes role, task binding and last activity. `whois` adds profile and recent mail archive commits. Registrations can outlive their runs. |
| tmux pane identities | `resolve_pane_identity`, `cleanup_pane_identities` | LM identifies a session by its run and agent. |
| Topics, delivery monitoring and read state | `fetch_topic`, `fetch_inbox_events`, `mark_message_read`, `mark_all_read`, `get_message_delivery_receipt` | Inbox polling and acknowledgements cover the default exchange. Enable these for topic consumers or delivery tracking. |
| Contact administration | `list_contacts`, `set_contact_policy` | The protocol requests and answers individual contacts. |
| Reservation overrides and Git hook installation | `force_release_file_reservation`, `install_precommit_guard`, `uninstall_precommit_guard` | LM releases its own reservations and installs its own commit check. |
| Mail history | `search_messages`, `summarize_thread` | Useful for investigating earlier work. The default workflow reads the inbox. |
| Convenience macros | `macro_prepare_thread`, `macro_file_reservation_cycle`, `macro_contact_handshake` | LM supplies the thread and uses the individual reservation and contact calls. |
| Cross-project products | `ensure_product`, `products_link`, `search_messages_product`, `fetch_inbox_product`, `summarize_thread_product` | Opt in with `multi_repo = true` to link repositories and query them together. |
| Build slots | `acquire_build_slot`, `renew_build_slot`, `release_build_slot` | The addon coordinates files. It does not schedule builds with Agent Mail. |
