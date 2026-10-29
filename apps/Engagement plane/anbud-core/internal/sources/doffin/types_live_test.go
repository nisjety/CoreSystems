package doffin

import (
	"context"
	"testing"
)

// Shapes observed on the live API on 2026-10-09 (values invented): result
// notices carry no status, timestamps and dates differ in precision, and the
// organisation number arrives with spaces.
func TestSearchDecodesTheShapesSeenOnTheLiveAPI(t *testing.T) {
	t.Parallel()

	const body = `{"numHitsTotal":159833,"numHitsAccessible":1000,"hits":[
	  {"id":"2026-100001","type":"ANNOUNCEMENT_OF_CONCLUSION_OF_CONTRACT",
	   "allTypes":["ANNOUNCEMENT_OF_CONCLUSION_OF_CONTRACT","RESULT"],
	   "status":null,"deadline":null,"issueDate":"2026-10-06T06:44:06Z","publicationDate":"2026-10-09",
	   "receivedTenders":null,"allReceivedTenders":null,"estimatedValue":null,
	   "limitedDataFlag":null,"doffinClassicUrl":null,
	   "buyer":[{"id":"415d965b4eb2955eb16e93599b5c7661","organizationId":"957 387 969","name":"Example IKS"}],
	   "locationId":["anyw"],"cpvCodes":["71300000"],
	   "heading":"h","description":"d","lots":[{"heading":"l","description":"d","winner":[]}]},
	  {"id":"2026-100002","type":"ANNOUNCEMENT_OF_COMPETITION",
	   "allTypes":["ANNOUNCEMENT_OF_COMPETITION","COMPETITION"],
	   "status":"ACTIVE","deadline":"2026-11-10T11:00:00Z","issueDate":"2026-10-07T13:32:11Z","publicationDate":"2026-10-09",
	   "receivedTenders":3,"allReceivedTenders":[{"type":"tenders","total":1},{"type":"t-esubm","total":1}],
	   "estimatedValue":{"currencyCode":"NOK","amount":7200000.0},
	   "buyer":[],"locationId":["NO081"],"cpvCodes":[],"heading":"h","description":"d","lots":[]}
	]}`
	c, _, _ := newTestClient(t, jsonHandler(body))

	resp, err := c.Search(context.Background(), SearchParams{})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}
	if len(resp.Hits) != 2 {
		t.Fatalf("hits = %d", len(resp.Hits))
	}

	result := resp.Hits[0]
	if result.Status != nil || result.Deadline != nil || result.EstimatedValue != nil {
		t.Errorf("a result notice has no status, deadline or value: %v / %v / %v", result.Status, result.Deadline, result.EstimatedValue)
	}
	if result.ReceivedTenders != 0 || len(result.AllReceivedTenders) != 0 {
		t.Errorf("null counts should decode as zero values: %d / %v", result.ReceivedTenders, result.AllReceivedTenders)
	}
	if got := result.Buyer[0].OrganizationID; got != "957 387 969" {
		t.Errorf("the organisation number is kept as sent for the normaliser to clean, got %q", got)
	}
	if len(result.AllTypes) != 2 || result.AllTypes[1] != "RESULT" {
		t.Errorf("allTypes carries the category after the concrete type: %v", result.AllTypes)
	}

	competition := resp.Hits[1]
	if competition.Status == nil || *competition.Status != "ACTIVE" {
		t.Errorf("status = %v", competition.Status)
	}
	if competition.EstimatedValue == nil || competition.EstimatedValue.Amount.String() != "7200000.0" {
		t.Errorf("estimatedValue = %+v", competition.EstimatedValue)
	}
	if competition.ReceivedTenders != 3 || len(competition.AllReceivedTenders) != 2 || competition.AllReceivedTenders[1].Type != "t-esubm" {
		t.Errorf("tender counts = %d / %+v", competition.ReceivedTenders, competition.AllReceivedTenders)
	}
}
