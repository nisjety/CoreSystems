package demo

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/incumbency"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/normalize"
	"github.com/triodelab/anbud/internal/noticeid"
	"github.com/triodelab/anbud/internal/rawstore"
	"golang.org/x/text/collate"
	"golang.org/x/text/language"
	"math/big"
	"sort"
	"strings"
	"time"
)

// CatalogueRequest contains public notice filters, never a company profile.
type CatalogueRequest struct {
	Cursor     string `json:"cursor"`
	Sort       string `json:"sort"`
	Direction  string `json:"direction"`
	NoticeType string `json:"notice_type"`
	Query      string `json:"query"`
	Buyer      string `json:"buyer"`
	Region     string `json:"region"`
	CPV        string `json:"cpv"`
	Status     string `json:"status"`
	Deadline   string `json:"deadline"`
}
type CatalogueNotice struct {
	Result
	MetadataAvailable bool                `json:"metadata_available"`
	ArrangementTypes  []string            `json:"arrangement_types"`
	Status            string              `json:"status"`
	IssuedAt          *time.Time          `json:"issued_at,omitempty"`
	AwardedValue      *Money              `json:"awarded_value,omitempty"`
	PublishedDate     string              `json:"published_date,omitempty"`
	Winners           []WinnerView        `json:"winners"`
	WinnersStatus     string              `json:"winners_status"`
	Contracts         []CatalogueContract `json:"contracts"`
}

type Catalogue struct {
	Results      []CatalogueNotice         `json:"results"`
	NextCursor   string                    `json:"next_cursor,omitempty"`
	TotalStored  *int                      `json:"total_stored,omitempty"`
	Scanned      int                       `json:"scanned"`
	TotalMatched int                       `json:"total_matched"`
	Facets       map[string]map[string]int `json:"facets"`
}
type catalogueSource interface {
	CataloguePage(context.Context, int64, int) ([]rawstore.CatalogueDocument, error)
}

var ErrCatalogueRequest = errors.New("invalid catalogue request")

// Catalogue sorts and counts the complete live corpus before paging. A scan that cannot
// finish within the service deadline fails explicitly instead of returning partial facet counts.
func (s *Service) Catalogue(ctx context.Context, r CatalogueRequest) (Catalogue, error) {
	out := Catalogue{Results: []CatalogueNotice{}}
	if len(r.Query) > 200 || len(r.Buyer) > 200 || len(r.Region) > 8 || len(r.CPV) > 10 || !validCatalogueOrder(r) {
		return out, ErrCatalogueRequest
	}
	if r.Status != "" && r.Status != "all" && r.Status != "active" && r.Status != "expired" {
		return out, ErrCatalogueRequest
	}
	if r.Deadline != "" && r.Deadline != "all" && r.Deadline != "soon" {
		return out, ErrCatalogueRequest
	}
	if r.NoticeType != "" && r.NoticeType != "all" && r.NoticeType != "competition" && r.NoticeType != "planning" && r.NoticeType != "result" && r.NoticeType != "other" {
		return out, ErrCatalogueRequest
	}
	source, ok := s.notices.(catalogueSource)
	if !ok {
		return out, errors.New("catalogue paging unavailable")
	}
	release, err := s.acquire()
	if err != nil {
		return out, err
	}
	defer release()
	scanCtx, cancel := context.WithTimeout(ctx, s.scanTimeout)
	defer cancel()
	var all []CatalogueNotice
	var revision int64
	if _, ok := s.notices.(projectionStore); ok {
		all, revision, out.Scanned, err = s.preparedCatalogue(scanCtx, r)
		if err != nil {
			return out, err
		}
	} else {
		all = []CatalogueNotice{}
		var copies []twin
		awardSets := map[string][]incumbency.Award{}
		before := int64(0)
		if r.Cursor != "" {
			cursor, err := readCatalogueCursor(r)
			if err != nil {
				return out, err
			}
			if cursor.Revision <= 0 || cursor.Revision == int64(^uint64(0)>>1) {
				return out, ErrCatalogueRequest
			}
			before = cursor.Revision + 1
		}
		revision = int64(0)
		for {
			docs, err := source.CataloguePage(scanCtx, before, 100)
			if err != nil {
				return out, err
			}
			for _, doc := range docs {
				if scanCtx.Err() != nil {
					return out, ErrTimeout
				}
				if doc.Sequence > revision {
					revision = doc.Sequence
				}
				before = doc.Sequence
				notice, c := catalogueNotice(doc.Document, s.now())
				all = append(all, notice)
				copies = append(copies, twin{noticeID: c.Notice.NoticeID, version: c.Notice.VersionID, source: doc.Document.Source})
				awardSets[notice.NoticeID] = incumbency.FromNotice(notice.NoticeID, c.Notice)
				out.Scanned++
			}
			if len(docs) < 100 {
				break
			}
		}
		// A notice held from Doffin and from TED is listed once (twins.go), before anything counts it.
		keep := keepOnePerNotice(copies)
		merged := all[:0]
		for i, n := range all {
			if keep[i] {
				merged = append(merged, n)
			} else {
				delete(awardSets, n.NoticeID)
			}
		}
		all = merged
		if err := s.enrichCatalogue(scanCtx, all, awardSets); err != nil {
			return out, err
		}

	}
	total := len(all)
	out.TotalStored = &total
	out.Facets = catalogueFacets(all, r)
	filtered := make([]CatalogueNotice, 0, len(all))
	for _, n := range all {
		if catalogueNoticeMatches(n, r) {
			filtered = append(filtered, n)
		}
	}
	out.TotalMatched = len(filtered)
	sortCatalogue(filtered, r.Sort, r.Direction)
	offset, err := decodeCatalogueCursor(r, revision)
	if err != nil {
		return out, err
	}
	if offset > len(filtered) {
		return out, ErrCatalogueRequest
	}
	end := min(offset+50, len(filtered))
	out.Results = filtered[offset:end]
	if end < len(filtered) {
		out.NextCursor = encodeCatalogueCursor(r, revision, end)
	}
	return out, nil
}

// searchFold is the form a search term and the text it is looked for in are compared in: lower case,
// every dash a hyphen-minus (a buyer types "–" in a title, a user types "-"), and every run of white
// space, a no-break space among them, one space.
func searchFold(s string) string {
	s = strings.Map(func(r rune) rune {
		switch r {
		case '\u2010', '\u2011', '\u2012', '\u2013', '\u2014', '\u2015', '\u2212', '\uFE58', '\uFE63', '\uFF0D':
			return '-'
		}
		return r
	}, strings.ToLower(s))
	return strings.Join(strings.Fields(s), " ")
}

func catalogueMatches(n Result, r CatalogueRequest, _ time.Time) bool {
	contains := func(text, term string) bool {
		return strings.Contains(searchFold(text), searchFold(term))
	}
	if !contains(n.Title+" "+n.Buyer, r.Query) || !contains(n.Buyer, r.Buyer) {
		return false
	}
	if r.Region != "" {
		found := false
		for _, v := range n.NUTS {
			found = found || strings.HasPrefix(strings.ToUpper(v), strings.ToUpper(r.Region))
		}
		if !found {
			return false
		}
	}
	if r.CPV != "" {
		found := false
		for _, v := range n.CPV {
			found = found || strings.HasPrefix(v, r.CPV)
		}
		if !found {
			return false
		}
	}
	if r.Status == "active" && (n.Deadline == nil || n.Deadline.DaysLeft < 0) {
		return false
	}
	if r.Status == "expired" && (n.Deadline == nil || n.Deadline.DaysLeft >= 0) {
		return false
	}
	return r.Deadline != "soon" || (n.Deadline != nil && n.Deadline.DaysLeft >= 0 && n.Deadline.DaysLeft <= 14)
}

// catalogueNotice shares the same typed metadata between list and detail, without company assessment.
func catalogueNotice(doc rawstore.Document, now time.Time) (CatalogueNotice, candidate) {
	c, kind, _ := read(doc)
	available := kind == kindOpportunity || kind == kindAward || kind == kindPlanning
	c.Norm = normalize.Notice(c.Notice)
	result := newResult(doc.ID, c, match.Assessment{}, match.Evidence{}, now)
	if !available {
		result = Result{NoticeID: doc.ID, Title: doc.ID, CPV: []string{}, NUTS: []string{}, BuyerOrgNumbers: []string{}, Gate: []CheckView{}, Fit: []CheckView{}, GateCounts: map[string]int{"pass": 0, "conditional": 0, "fail": 0, "unknown": 0}}
		if header, err := eforms.Sniff(doc.Body); err == nil {
			result.NoticeType = header.NoticeTypeCode
		}
	}
	result.Verdict = "unknown"
	result.Band = "none"
	if result.Deadline == nil && len(c.Norm.Deadlines) > 0 {
		d := c.Norm.Deadlines[0]
		result.Deadline = &DeadlineView{At: d.At.UTC(), OsloDay: normalize.OsloDay(d.At), DaysLeft: normalize.DaysLeft(d.At, now), Kind: string(d.Kind), Lot: d.Lot}
	}
	arrangements := []string{}
	seen := map[string]bool{}
	for _, system := range c.Notice.ContractingSystems {
		value := ""
		switch {
		case system.Kind == "framework-agreement" && (system.Code == "fa-mix" || system.Code == "fa-wo-rc" || system.Code == "fa-w-rc"):
			value = "framework"
		case system.Kind == "dps-usage" && (system.Code == "dps-list" || system.Code == "dps-nlist"):
			value = "dps"
		}
		if value != "" && !seen[value] {
			seen[value] = true
			arrangements = append(arrangements, value)
		}
	}
	status := "unknown"
	switch {
	case strings.HasPrefix(result.NoticeType, "pin-"):
		status = "planning"
	case strings.HasPrefix(result.NoticeType, "can-"):
		status = "result"
	case result.Deadline != nil && result.Deadline.DaysLeft < 0:
		status = "expired"
	case result.Deadline != nil:
		status = "active"
	}
	contracts := []CatalogueContract{}
	awards := incumbency.FromNotice(doc.ID, c.Notice)
	for _, lot := range c.Notice.Lots {
		contract := CatalogueContract{Lot: lot.ID, Title: clip(lot.Title, maxTextRunes), EndBasis: "unknown"}
		if lot.Period != nil {
			if lot.Period.Duration != nil {
				contract.Duration = &CatalogueDuration{Value: lot.Period.Duration.Value, Unit: lot.Period.Duration.Unit}
			}
			contract.Unlimited = lot.Period.Unlimited
			if lot.Period.Start != nil {
				contract.StartDate = dateOf(lot.Period.Start)
			}
			award := incumbency.Award{Period: lot.Period}
			for _, a := range awards {
				if a.Lot == lot.ID {
					award = a
					break
				}
			}
			end := award.End()
			contract.EndDate = dateOf(end.At)
			contract.EndBasis = string(end.Basis)
		}
		contracts = append(contracts, contract)
	}
	return CatalogueNotice{Contracts: contracts, Winners: []WinnerView{}, WinnersStatus: winnersNotConfigured, Result: result, MetadataAvailable: available, ArrangementTypes: arrangements, Status: status, IssuedAt: utc(c.Notice.IssuedAt), AwardedValue: money(c.Notice.AwardedValue)}, c
}

type CatalogueDetail struct {
	Result               CatalogueNotice `json:"result"`
	Notice               NoticeView      `json:"notice"`
	Evidence             Evidence        `json:"evidence"`
	DescriptionTruncated bool            `json:"description_truncated"`
}

// CatalogueDetail reads the latest live body of one notice. It never reads customer uploads.
func (s *Service) CatalogueDetail(ctx context.Context, id string) (CatalogueDetail, error) {
	var out CatalogueDetail
	if !noticeid.Valid(id) {
		return out, ErrInvalidNoticeID
	}
	release, err := s.acquire()
	if err != nil {
		return out, err
	}
	defer release()
	readCtx, cancel := context.WithTimeout(ctx, s.scanTimeout)
	defer cancel()
	doc, err := s.notices.Notice(readCtx, id)
	if errors.Is(err, rawstore.ErrNotFound) {
		return out, ErrNotFound
	}
	if err != nil {
		return out, err
	}
	result, c := catalogueNotice(doc, s.now())
	enriched := []CatalogueNotice{result}
	if err := s.enrichCatalogue(readCtx, enriched, map[string][]incumbency.Award{id: incumbency.FromNotice(id, c.Notice)}); err != nil {
		return out, err
	}
	result = enriched[0]
	audit := newAudit(doc, c, match.Assessment{}, match.Evidence{}, s.now())
	audit.Notice.Description = clip(c.Notice.Description, 30000)
	return CatalogueDetail{Result: result, Notice: audit.Notice, Evidence: audit.Evidence, DescriptionTruncated: len([]rune(c.Notice.Description)) > 30000}, nil
}

type CatalogueDuration struct {
	Value int    `json:"value"`
	Unit  string `json:"unit"`
}

type CatalogueContract struct {
	Lot       string             `json:"lot"`
	Title     string             `json:"title,omitempty"`
	StartDate string             `json:"start_date,omitempty"`
	EndDate   string             `json:"end_date,omitempty"`
	EndBasis  string             `json:"end_basis"`
	Duration  *CatalogueDuration `json:"duration,omitempty"`
	Unlimited bool               `json:"unlimited"`
}

func (s *Service) enrichCatalogue(ctx context.Context, notices []CatalogueNotice, sets map[string][]incumbency.Award) error {
	if source, ok := s.notices.(interface {
		PublicationDates(context.Context) (map[string]string, error)
	}); ok {
		dates, err := source.PublicationDates(ctx)
		if err != nil {
			return err
		}
		for i := range notices {
			notices[i].PublishedDate = dates[notices[i].NoticeID]
		}
	}
	s.enrichCatalogueWinners(ctx, notices, sets)
	return nil
}

func (s *Service) enrichCatalogueWinners(ctx context.Context, notices []CatalogueNotice, sets map[string][]incumbency.Award) {
	awards := []incumbency.Award{}
	for _, n := range notices {
		awards = append(awards, sets[n.NoticeID]...)
	}
	classes, state := s.classify(ctx, incumbency.NorwegianWinners(awards, maxWinnerLookups))
	for i := range notices {
		notices[i].WinnersStatus = state
		seen := map[string]bool{}
		for _, entry := range incumbency.Show(sets[notices[i].NoticeID], classes) {
			for _, w := range awardView(entry).Winners {
				key := w.OrgNumber + "/" + w.Name + "/" + w.Status
				if !seen[key] {
					seen[key] = true
					notices[i].Winners = append(notices[i].Winners, w)
				}
			}
		}
	}
}
func catalogueKind(n CatalogueNotice) string {
	if strings.HasPrefix(n.NoticeType, "cn-") {
		return "competition"
	}
	if strings.HasPrefix(n.NoticeType, "pin-") || n.NoticeType == "pmc" {
		return "planning"
	}
	// A voluntary ex-ante transparency notice (veat) says whom the buyer means to award without a
	// competition: an award notice by its root, a result by its kind.
	if strings.HasPrefix(n.NoticeType, "can-") || n.NoticeType == "veat" {
		return "result"
	}
	return "other"
}
func catalogueNoticeMatches(n CatalogueNotice, r CatalogueRequest) bool {
	return catalogueMatches(n.Result, r, time.Time{}) && (r.NoticeType == "" || r.NoticeType == "all" || catalogueKind(n) == r.NoticeType)
}
func catalogueFacets(all []CatalogueNotice, r CatalogueRequest) map[string]map[string]int {
	out := map[string]map[string]int{"status": {"all": 0, "active": 0, "expired": 0}, "deadline": {"all": 0, "soon": 0}, "noticeType": {"all": 0, "competition": 0, "planning": 0, "result": 0, "other": 0}, "buyer": {}, "region": {}, "cpv": {}}
	for _, n := range all {
		base := r
		base.Status = "all"
		if catalogueNoticeMatches(n, base) {
			out["status"]["all"]++
			if n.Deadline != nil {
				if n.Deadline.DaysLeft < 0 {
					out["status"]["expired"]++
				} else {
					out["status"]["active"]++
				}
			}
		}
		base = r
		base.Deadline = "all"
		if catalogueNoticeMatches(n, base) {
			out["deadline"]["all"]++
			if n.Deadline != nil && n.Deadline.DaysLeft >= 0 && n.Deadline.DaysLeft <= 14 {
				out["deadline"]["soon"]++
			}
		}
		base = r
		base.Buyer = ""
		if catalogueNoticeMatches(n, base) && n.Buyer != "" {
			out["buyer"][n.Buyer]++
		}
		base = r
		base.Region = ""
		if catalogueNoticeMatches(n, base) {
			seen := map[string]bool{}
			for _, code := range n.NUTS {
				if !seen[code] {
					seen[code] = true
					out["region"][code]++
				}
			}
		}
		base = r
		base.CPV = ""
		if catalogueNoticeMatches(n, base) {
			seen := map[string]bool{}
			for _, code := range n.CPV {
				if !seen[code] {
					seen[code] = true
					out["cpv"][code]++
				}
			}
		}
		base = r
		base.NoticeType = "all"
		if catalogueNoticeMatches(n, base) {
			out["noticeType"]["all"]++
			out["noticeType"][catalogueKind(n)]++
		}
	}
	return out
}

var catalogueOrders = map[string]bool{"title": true, "buyer": true, "noticeType": true, "arrangement": true, "status": true, "estimatedValue": true, "awardedValue": true, "deadline": true, "published": true, "issued": true, "winners": true, "duration": true, "endDate": true, "match": true}

func validCatalogueOrder(r CatalogueRequest) bool {
	return (r.Sort == "" || catalogueOrders[r.Sort]) && (r.Direction == "" || r.Direction == "asc" || r.Direction == "desc")
}
func sortCatalogue(rows []CatalogueNotice, key, direction string) {
	if key == "" {
		key = "deadline"
	}
	// x/text has no Norwegian tailoring; Danish supplies the shared Æ, Ø, Å ordering.
	collator := collate.New(language.Danish)
	sort.SliceStable(rows, func(i, j int) bool {
		a, aok := catalogueSortValue(rows[i], key)
		b, bok := catalogueSortValue(rows[j], key)
		if aok != bok {
			return aok
		}
		if !aok {
			return rows[i].NoticeID < rows[j].NoticeID
		}
		cmp := collator.CompareString(a, b)
		if key == "estimatedValue" || key == "awardedValue" || key == "duration" {
			aa, aok := new(big.Rat).SetString(a)
			bb, bok := new(big.Rat).SetString(b)
			if aok && bok {
				cmp = aa.Cmp(bb)
			}
		}
		if cmp == 0 {
			return rows[i].NoticeID < rows[j].NoticeID
		}
		if direction == "desc" {
			return cmp > 0
		}
		return cmp < 0
	})
}
func catalogueSortValue(n CatalogueNotice, key string) (string, bool) {
	value := ""
	switch key {
	case "title":
		value = strings.ToLower(n.Title)
	case "buyer":
		value = strings.ToLower(n.Buyer)
	case "noticeType":
		switch catalogueKind(n) {
		case "competition":
			value = "konkurranse"
		case "planning":
			value = "veiledende kunngjøring"
		case "result":
			value = "resultat"
		default:
			value = n.NoticeType
		}
	case "arrangement":
		value = strings.Join(n.ArrangementTypes, ",")
	case "status":
		value = map[string]string{"active": "aktiv", "expired": "utgått", "planning": "planlagt", "result": "resultat"}[n.Status]
	case "estimatedValue":
		if n.EstimatedValue != nil {
			value = n.EstimatedValue.Amount
		}
	case "awardedValue":
		if n.AwardedValue != nil {
			value = n.AwardedValue.Amount
		}
	case "deadline":
		if n.Deadline != nil {
			value = n.Deadline.At.UTC().Format(time.RFC3339)
		}
	case "published":
		value = n.PublishedDate
	case "issued":
		if n.IssuedAt != nil {
			value = n.IssuedAt.UTC().Format(time.RFC3339)
		}
	case "winners":
		for _, w := range n.Winners {
			if w.Name != "" {
				value = strings.ToLower(w.Name)
				break
			}
		}
	case "duration":
		for _, c := range n.Contracts {
			if c.Duration != nil {
				factor := map[string]float64{"DAY": 1, "WEEK": 7, "MONTH": 30.4375, "YEAR": 365.25}[c.Duration.Unit]
				value = fmt.Sprint(float64(c.Duration.Value) * factor)
				break
			}
		}
	case "endDate":
		for _, c := range n.Contracts {
			if c.EndDate != "" && (value == "" || c.EndDate < value) {
				value = c.EndDate
			}
		}
	case "match":
		value = n.Band
	}
	return value, value != ""
}

type catalogueCursor struct {
	Offset   int    `json:"offset"`
	Key      string `json:"key"`
	Revision int64  `json:"revision"`
}

func catalogueRequestKey(r CatalogueRequest) string {
	r.Cursor = ""
	body, _ := json.Marshal(r)
	return fmt.Sprintf("%x", sha256.Sum256(body))
}
func encodeCatalogueCursor(r CatalogueRequest, revision int64, offset int) string {
	body, _ := json.Marshal(catalogueCursor{offset, catalogueRequestKey(r), revision})
	return base64.RawURLEncoding.EncodeToString(body)
}
func decodeCatalogueCursor(r CatalogueRequest, revision int64) (int, error) {
	if r.Cursor == "" {
		return 0, nil
	}
	if len(r.Cursor) > 512 {
		return 0, ErrCatalogueRequest
	}
	body, err := base64.RawURLEncoding.DecodeString(r.Cursor)
	if err != nil {
		return 0, ErrCatalogueRequest
	}
	var c catalogueCursor
	if json.Unmarshal(body, &c) != nil || c.Offset < 0 || c.Key != catalogueRequestKey(r) || c.Revision != revision {
		return 0, ErrCatalogueRequest
	}
	return c.Offset, nil
}

func readCatalogueCursor(r CatalogueRequest) (catalogueCursor, error) {
	var c catalogueCursor
	if len(r.Cursor) > 512 {
		return c, ErrCatalogueRequest
	}
	body, err := base64.RawURLEncoding.DecodeString(r.Cursor)
	if err != nil {
		return c, ErrCatalogueRequest
	}
	if json.Unmarshal(body, &c) != nil || c.Offset < 0 || c.Key != catalogueRequestKey(r) {
		return c, ErrCatalogueRequest
	}
	return c, nil
}
