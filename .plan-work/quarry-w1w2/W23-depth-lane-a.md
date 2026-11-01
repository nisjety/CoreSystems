# W23 depth findings, lane A

W2-U16 | D2-U16-1 | SchedulesStore.List (unscoped, all orgs) stays in the store interface and the Postgres store though no handler calls it now; only tests use it. Remove it or rename it to make the all-orgs reach explicit. | services/quarry-control/internal/store/store.go | low
W2-U16 | D2-U16-2 | The retired QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG constant and compose line can go once no pre-D2-042 control image can be rolled back to. | docker-compose.yml, resources/schedules.go | low
W2-U16 | D2-U16-3 | The Postgres scoping test sits in the resources package and re-implements the QUARRY_TEST_DSN gate from store/pg; share the gate through storetest. | resources/schedules_test.go | low
