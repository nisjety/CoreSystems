# Velion Refactoring - Implementation Checklist

## Phase 1: Foundation Setup (Days 1-2)

### A. Environment & Dependencies
- [x] All files created
- [ ] Run `pnpm install` to verify no new dependencies needed
- [ ] Run `pnpm build` to check for TypeScript errors
- [ ] Verify no import errors in IDE

### B. Root Layout Integration
- [ ] Open `src/app/layout.tsx`
- [ ] Add SessionProvider wrapper
- [ ] Test app loads without errors
- [ ] Verify session context available app-wide
- [ ] Test redirect to login works

### C. Verify Core Utilities
- [ ] Import `THEME` from `@/lib/design-tokens` in any component
- [ ] Import `apiGet` from `@/lib/api-utils` 
- [ ] Import `useSession` from `@/lib/session-context`
- [ ] Verify all compile without errors

## Phase 2: Migrate Dashboard Pages (Days 3-4)

### A. Projects Page (`dashboard/projects/page.tsx`)
- [ ] Update to use `MasterPageLayout`
- [ ] Use `GridContainer` for layout
- [ ] Replace manual cards with `StatCard`
- [ ] Add loading state with `LoadingState`
- [ ] Add error state with `ErrorState`
- [ ] Use `useFetch` for data
- [ ] Test page loads and displays correctly

### B. Tasks Page (`dashboard/tasks/page.tsx`)
- [ ] Repeat Projects page migration steps
- [ ] Test task list displays correctly

### C. Agents Page (`dashboard/agents/page.tsx`)
- [ ] Repeat Projects page migration steps
- [ ] Test agents display correctly

### D. Other Main Sections (Days 5-6)
- [ ] Deploy/Deployment page
- [ ] Helpdesk page
- [ ] Inbox page
- [ ] Outbound page
- [ ] People page
- [ ] Reports page

### E. Verification Tasks
- [ ] No console errors on any page
- [ ] All pages have consistent styling
- [ ] Loading states display correctly
- [ ] Error states display correctly
- [ ] Empty states display correctly
- [ ] All redirects work

## Phase 3: Component Replacement (Days 7-9)

### A. Replace Duplicate Components
- [ ] Find all MetricCard usages in `components/overview`
- [ ] Replace with `StatCard` from shared
- [ ] Find all card components scattered across sections
- [ ] Replace with `CommonCard` from shared
- [ ] Delete old component files

### B. Update API Calls
- [ ] Replace all `fetch()` calls with `apiGet/apiPost`
- [ ] Update error handling to use centralized system
- [ ] Add retry logic where needed
- [ ] Test API calls work correctly

### C. Standardize Forms
- [ ] Find all form implementations
- [ ] Create `CommonForm` component
- [ ] Replace duplicate form code with `CommonForm`
- [ ] Delete old form files

## Phase 4: Optional Enhancements (Days 10-12)

### A. Create CommonTable
- [ ] Create `src/components/shared/CommonTable.tsx`
- [ ] Support multiple column types
- [ ] Support sorting and pagination
- [ ] Replace list view components

### B. Create CommonModal
- [ ] Create `src/components/shared/CommonModal.tsx`
- [ ] Support custom content
- [ ] Support submit/cancel actions
- [ ] Replace all modal implementations

### C. Create CommonForm
- [ ] Create `src/components/shared/CommonForm.tsx`
- [ ] Support text, email, password, textarea, select
- [ ] Support validation
- [ ] Support error messages

### D. Theme Enhancements
- [ ] Add dark mode support to THEME
- [ ] Add animation presets
- [ ] Add responsive breakpoints
- [ ] Document theme extension

## Phase 5: Testing & QA (Days 13-14)

### A. Functionality Tests
- [ ] Dashboard page loads
- [ ] All sections accessible
- [ ] Data displays correctly
- [ ] Create/update works
- [ ] Delete works
- [ ] Filtering works
- [ ] Search works
- [ ] Pagination works

### B. Styling Consistency
- [ ] Check all colors match THEME
- [ ] Check all spacing matches THEME
- [ ] Check all typography matches THEME
- [ ] Check all shadows match THEME
- [ ] No inline styles except needed overrides
- [ ] Responsive design works on mobile/tablet/desktop

### C. Performance
- [ ] Load dashboard page (check performance)
- [ ] Load list with 100+ items
- [ ] Filter/search performance
- [ ] Memory usage (no leaks)
- [ ] Network requests optimized

### D. Accessibility
- [ ] Keyboard navigation works
- [ ] Screen reader support
- [ ] Color contrast sufficient
- [ ] Focus states visible
- [ ] Error messages clear

### E. Error Scenarios
- [ ] Test API timeout
- [ ] Test 404 error
- [ ] Test 500 error
- [ ] Test network disconnected
- [ ] Test invalid session
- [ ] Test permission denied

## Phase 6: Documentation & Cleanup (Days 15-16)

### A. Code Documentation
- [ ] Add JSDoc comments to all shared components
- [ ] Document design tokens usage
- [ ] Document API utils conventions
- [ ] Document hook usage patterns
- [ ] Create component prop examples

### B. Repository Cleanup
- [ ] Remove all old/duplicate files
- [ ] Remove unused imports
- [ ] Format code with `pnpm lint`
- [ ] Run TypeScript check: `pnpm build`
- [ ] Verify production build: `pnpm build && pnpm start`

### C. Create Examples
- [ ] Create example pages folder
- [ ] Add example using all shared components
- [ ] Add example with forms and modals
- [ ] Add example with data fetching
- [ ] Add example with error handling

### D. Update Main Documentation
- [ ] Update README with new structure
- [ ] Add quick start guide
- [ ] Add troubleshooting section
- [ ] Add FAQ section
- [ ] Link to all doc files

## Phase 7: Team Onboarding (Days 17-18)

### A. Training Materials
- [ ] Record video tour of new architecture
- [ ] Create architecture overview slides
- [ ] Create coding standards guide
- [ ] Create troubleshooting guide

### B. Code Review Setup
- [ ] Create PR template
- [ ] Add linting rules
- [ ] Add TypeScript strict mode
- [ ] Add pre-commit hooks

### C. Handoff
- [ ] Team review of changes
- [ ] Q&A session
- [ ] Best practices walkthrough
- [ ] Common pitfalls discussion

## Daily Checklist Template

```
Date: ___________

Morning:
- [ ] Code compiles (pnpm build)
- [ ] No console errors
- [ ] All tests pass (if any)
- [ ] Dependencies up to date

Afternoon:
- [ ] Migrated X pages/components
- [ ] Updated Y API calls
- [ ] Fixed Z issues
- [ ] Documented changes

Before Commit:
- [ ] Run formatter (pnpm lint)
- [ ] Run TypeScript check
- [ ] Test in browser
- [ ] Commit message written
```

## Troubleshooting

### Build Errors
```bash
# Clear cache and rebuild
rm -rf .next
pnpm build
```

### TypeScript Errors
```bash
# Check for type issues
pnpm build --verbose
```

### Import Errors
```bash
# Verify barrel exports
cat src/components/shared/index.ts

# Try explicit import
import { Component } from '@/components/shared/ComponentName'
```

### Runtime Errors
```bash
# Check browser console
# Check server logs
# Check API responses in Network tab
```

## Success Criteria

✅ All dashboard pages use MasterPageLayout  
✅ All components use unified THEME  
✅ All API calls use api-utils  
✅ Zero code duplication for UI components  
✅ All TypeScript types defined  
✅ All pages have proper loading/error states  
✅ Consistent styling across all sections  
✅ Type coverage >95%  
✅ Bundle size reduced by 10-15%  
✅ Development velocity increased  

## Sign Off

- [ ] Code Review Complete
- [ ] QA Testing Complete
- [ ] Documentation Complete
- [ ] Team Training Complete
- [ ] Production Ready

---

**Started:** April 4, 2026  
**Target Complete:** April 18, 2026  
**Actual Complete:** ___________

