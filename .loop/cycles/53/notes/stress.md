# Stress cycle 53 (8x yes load, 12 parallel)
A (default threads): 300 runs, 3 failures (2, 33, 156). B (--test-threads 4): 200 runs, 0 failures. All failures in services binary; service_ports never failed.

## run 2
thread 'a_service_with_an_http_check_is_ready_once_it_answers' (1311961) panicked at crates/aae-testkit/tests/services.rs:242:9:
{"failure":{"account":null,"kind":"tool_failure","provider_model":null,"raw":"mcp-probe: Address already in use (os error 98)\n","retried_by_harness":false,"retry_at":null,"scope":"session","source":"core","text":null},"service":{"id":"s-00dc8327d1e99c5f","name":"probe","pgid":null,"pid":null,"scope":"orchestrator"}}
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    a_service_with_an_http_check_is_ready_once_it_answers

test result: FAILED. 8 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.98s

--- listeners block:

## run 33
thread 'a_service_with_an_http_check_is_ready_once_it_answers' (1315299) panicked at crates/aae-testkit/tests/services.rs:242:9:
{"failure":{"account":null,"kind":"tool_failure","provider_model":null,"raw":"mcp-probe: Address already in use (os error 98)\n","retried_by_harness":false,"retry_at":null,"scope":"session","source":"core","text":null},"service":{"id":"s-2661b72898d75077","name":"probe","pgid":null,"pid":null,"scope":"orchestrator"}}
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    a_service_with_an_http_check_is_ready_once_it_answers

test result: FAILED. 8 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.90s

--- listeners block:

## run 156
thread 'one_service_serves_every_session_and_restarts_on_its_port' (1331011) panicked at crates/aae-testkit/tests/services.rs:74:5:
assertion `left == right` failed: the same URL works again
  left: 0
 right: 200
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    one_service_serves_every_session_and_restarts_on_its_port

test result: FAILED. 8 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.51s

--- listeners block:

## Повтор на bdd801f
CI: EXIT 0, 327 tests run: 327 passed. A: 300 runs, 2 failures (99, 159). B: 200 runs, 0 failures.

### run 99 (r-99.svc.log)
thread 'one_service_serves_every_session_and_restarts_on_its_port' (1449353) panicked at crates/aae-testkit/tests/services.rs:92:5:
assertion `left != right` failed
  left: 1449376
 right: 1449376
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    one_service_serves_every_session_and_restarts_on_its_port

test result: FAILED. 8 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.32s


### run 159 (r-159.svc.log)
thread 'one_service_serves_every_session_and_restarts_on_its_port' (1457383) panicked at crates/aae-testkit/tests/services.rs:92:5:
assertion `left != right` failed
  left: 1457398
 right: 1457398
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    one_service_serves_every_session_and_restarts_on_its_port

test result: FAILED. 8 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.34s
