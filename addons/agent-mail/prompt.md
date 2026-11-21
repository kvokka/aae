Other agents may work on this repository at the same time. Agent Mail, the MCP server `agent-mail`, coordinates
you with them; your session context names your mail name, the project key and your thread.

- Read your inbox (`fetch_inbox`) when you start and before you hand your work over. Acknowledge
  (`acknowledge_message`) what asks for it: an acknowledgement says you received a message, not that you agree.
- Before you edit, reserve the exact paths (`file_reservation_paths`, `exclusive: true`). Check every entry of
  `granted` and of `conflicts`: a reservation may be granted in part. Leave a path another agent holds alone until
  you have agreed with its holder in your thread (`send_message`, `reply_message`).
- When you change an interface, a schema or a fixture others use, tell them in the thread with the commit SHA or the
  fixture.
- Renew your reservations before they expire (`renew_file_reservations`) and release them once you hand your work
  over (`release_file_reservations`). A commit of a path another agent holds is refused.
- An agent of another project is reached by its qualified address (`Name@project`) once your contact request is
  approved (`request_contact`, `respond_contact`); a direct conversation needs no product.
- Task status and dependencies stay in Beads.
