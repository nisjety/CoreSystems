/** Site-scoped capabilities shared by Verevon and future governed agent adapters. */
export const visibilityToolViews = ['pages', 'groups', 'links', 'competitors'] as const
export type VisibilityToolView = typeof visibilityToolViews[number]
export const visibilityToolsContracts = {
 collection: { id:'visibility.collection.inventory.read', owner:'visibility-core', method:'GET', path:'/api/v1/visibility/collection', readOnly:true, providerCollection:false },
 googleResources:{id:"visibility.website.google.resources",owner:"visibility-core",method:"GET",path:"/api/v1/visibility/websites/:websiteId/google/resources",readOnly:true},
 googleSelect:{id:"visibility.website.google.select",owner:"visibility-core",method:"POST",path:"/api/v1/visibility/websites/:websiteId/google/resources",allowedActorTypes:["human"],risk:"low",authority:"integration-core-current-resource-discovery"},
 review: {id:"visibility.work.search-review.save",owner:"visibility-core",method:"POST",path:"/api/v1/visibility/work/review",allowedActorTypes:["human"],risk:"low",evidence:"server-saved-search-group"},
 plan: {id:"visibility.work.plan",owner:"visibility-core",method:"PATCH",path:"/api/v1/visibility/work/:taskId/planning",allowedActorTypes:["human"],risk:"low",assigneesAuthority:"control-plane"},
 research: {id:"visibility.search.research.read",owner:"visibility-core",method:"GET",path:"/api/v1/visibility/search/research",readOnly:true},
 researchRefresh: {id:"visibility.search.research.refresh",owner:"visibility-core",method:"POST",path:"/api/v1/visibility/search/research",allowedActorTypes:["human"],risk:"low"},
 researchRules: {id:"visibility.search.groups.configure",owner:"visibility-core",method:"POST",path:"/api/v1/visibility/search/rules",allowedActorTypes:["human"],risk:"low"},
 analysis: { id: 'visibility.analysis.read', owner: 'visibility-core', method: 'GET', path: '/api/v1/visibility/analysis', readOnly: true },
 competitors: { id: 'visibility.competitors.read', owner: 'visibility-core', method: 'GET', path: '/api/v1/visibility/competitors', readOnly: true },
 compare: { id: 'visibility.competitors.refresh', owner: 'visibility-core', method: 'POST', path: '/api/v1/visibility/competitors', allowedActorTypes: ['human'], risk: 'low', operations: ['refresh', 'remove'], publicOnly: true, maxTargets: 3,maxPagePairsPerTarget:5 },
} as const
