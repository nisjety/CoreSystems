package main

import "testing"

func TestCronFireAuthorizerFromEnvFailsClosedWhenDeploymentMaterialIsMissing(t *testing.T) {
	if _, err := cronFireAuthorizerFromEnv(func(string) string { return "" }); err == nil {
		t.Fatal("cron scheduler authorizer accepted an empty deployment configuration")
	}
}

func TestCronDecisionVerifierFromEnvFailsClosedWhenPublicKeyIsMissing(t *testing.T) {
	if _, err := cronDecisionVerifierFromEnv(func(string) string { return "" }); err == nil {
		t.Fatal("cron schedule creation verifier accepted an empty deployment configuration")
	}
}
