# WP-121: ручные мутации (test(config_subagents))

Все 20 убиты, выживших и невалидных нет. Все тесты — tests::config_subagents::*.

| # | результат | упавшие тесты |
|---|---|---|
| 1 | killed | 7: a_variant_has_a_list_of_its_own, the_strongest_layer_replaces_the_list, explain_prints_the_resolved_list, no_list_means_only_the_agent_itself, a_name_is_bare_or_qualified, a_pattern_matches_agents_and_variants, a_rig_puts_its_variant_in_place_of_a_named_agent |
| 2 | killed | a_name_is_bare_or_qualified, the_strongest_layer_replaces_the_list, a_variant_has_a_list_of_its_own, a_rig_puts_its_variant_in_place_of_a_named_agent |
| 3 | killed | a_pattern_matches_agents_and_variants |
| 4 | killed | a_pattern_matches_agents_and_variants |
| 5 | killed | a_rig_puts_its_variant_in_place_of_a_named_agent |
| 6 | killed | a_rig_puts_its_variant_in_place_of_a_named_agent |
| 7 | killed | a_built_in_agent_type_is_no_name_of_a_target |
| 8 | killed | a_target_of_another_harness_is_an_error |
| 9 | killed | a_target_of_another_scope_or_host_is_an_error |
| 10 | killed | a_target_of_another_scope_or_host_is_an_error |
| 11 | killed | a_target_needs_a_description |
| 12 | killed | 11+ (почти все: an_env_key_with_two_values_is_an_error, a_target_needs_a_description, ...) |
| 13 | killed | a_target_plugin_for_another_harness_is_an_error |
| 14 | killed | 6: an_env_key_with_two_values_is_an_error, a_target_plugin_for_another_harness_is_an_error, a_target_needs_a_description, a_built_in_agent_type_is_no_name_of_a_target, a_target_of_another_scope_or_host_is_an_error, a_name_nobody_has_is_an_error |
| 15 | killed | a_name_nobody_has_is_an_error |
| 16 | killed | a_target_of_another_harness_is_an_error |
| 17 | killed | the_strongest_layer_replaces_the_list, a_variant_has_a_list_of_its_own |
| 18 | killed | explain_prints_the_resolved_list |
| 19 | killed | 11+ (почти все: тесты требуют description) |
| 20 | killed | the_keys_belong_to_an_agent |
