p='work.rs'
s=open(p,encoding='utf-8',newline='').read()
nl='\n'
a='    let view = page_view(&target.host, &observation);'
assert s.count(a)==1
s=s.replace(a,'    let view = page_view(&target.host, &observation, false);')
old=nl.join(['    run_id: &str,',') {','    end_hold(run_id);','    forget_help(run_id);','    let Some(session) = state.take_browser_session(run_id) else {','        return;','    };','    let quarry = QuarryAgentClient::from_env().ok().flatten();',''])
assert s.count(old)==1, s.count(old)
new=nl.join(['    run_id: &str,',') {',
'    let quarry = || QuarryAgentClient::from_env().ok().flatten();',
'    release_run_with(state, broker, quarry, bearer, org_id, run_id).await;',
'}','',
"/// [`release_run`] with the Quarry client given, built only for a run that holds a page. A run that",
"/// ends after a hand-back closes the Quarry run like any other, so the person's login ends with it",
'/// and with its revoked grant (EP-1 D8).',
'async fn release_run_with(',
'    state: &StateStore,',
'    broker: &dyn BrowserBroker,',
'    quarry: impl FnOnce() -> Option<QuarryAgentClient>,',
'    bearer: &str,',
'    org_id: &str,',
'    run_id: &str,',
') {',
'    take_hold(run_id);',
'    forget_typed_by_person(run_id);',
'    forget_help(run_id);',
'    let Some(session) = state.take_browser_session(run_id) else {',
'        return;','    };',
'    let quarry = quarry();',''])
s=s.replace(old,new)
open(p,'w',encoding='utf-8',newline='').write(s)
