/** Contract-test evidence only. Never imported by the running application. */
import type { CollectionInventory, Dataset } from '../collection-client'
import { context } from './tools'
const row=(key:string):Dataset=>({key,provider:'google_search_console',category:'account',status:'pending',coverage:'sampled_queries',partial:true,records:null,unit:'query_rows',history:'latest_snapshot',collectionMode:'scheduled'})
export const inventory=():CollectionInventory=>({...context,source:'visibility_collection_inventory',asOf:'2026-10-29T10:00:00Z',quarter:'Q4',datasets:[{...row('gsc'),status:'empty',records:0,lastSuccessAt:'2026-10-29T09:00:00Z'},row('ga4'),{...row('backlink_index'),provider:'external_source',category:'market',status:'unsupported',coverage:'external_source_required',reason:'additional_source_required',collectionMode:'unconfigured',history:'none'}],summary:{available:0,empty:1,pending:1,stale:0,attention:0,unsupported:1}})
