import { it,expect } from 'vitest'
import { sanitizeSEO } from './seo-evidence'
const site='https://triodelab.no/'
const evidence=()=>({version:'seo_page_v1',headings:[{level:1,text:'Public heading'}],headingsPartial:false,noindex:false,nofollow:false,robotsStatus:200,wordCount:5,alternates:[],invalidAlternates:0,alternatesPartial:false,redirects:[],canonicalOutsideSite:false,structuredData:{documents:1,invalid:0,types:['Organization'],partial:false}})
it('projects structural observations without raw schema properties',()=>{const v=evidence();expect(sanitizeSEO({...v,html:'secret',structuredData:{...v.structuredData,raw:'secret'}},site)).toEqual(v)})
it('rejects incomplete directives, unsafe redirects and invalid heading levels',()=>{const v=evidence();for(const bad of [{...v,noindex:undefined},{...v,redirects:[{url:site,status:302,target:'https://other.no/'}]},{...v,headings:[{level:7,text:'bad'}]},{...v,wordCount:-1}])expect(()=>sanitizeSEO(bad,site)).toThrow()})
