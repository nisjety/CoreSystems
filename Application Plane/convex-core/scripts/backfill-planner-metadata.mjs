import { ConvexHttpClient } from 'convex/browser';
import { makeFunctionReference } from 'convex/server';

const backfillMetadataRef = makeFunctionReference('plannerDocuments:backfillMetadata');

function getArgValue(flagName) {
  const argumentIndex = process.argv.indexOf(flagName);

  if (argumentIndex === -1) {
    return undefined;
  }

  return process.argv[argumentIndex + 1];
}

function hasFlag(flagName) {
  return process.argv.includes(flagName);
}

function getConvexUrl() {
  return (
    process.env.CONVEX_SELF_HOSTED_URL ||
    process.env.NEXT_PUBLIC_CONVEX_URL ||
    process.env.CONVEX_URL ||
    'http://127.0.0.1:3210'
  );
}

async function main() {
  const client = new ConvexHttpClient(getConvexUrl(), {
    skipConvexDeploymentUrlCheck: true,
    logger: false,
  });
  const workspaceId = getArgValue('--workspace') ?? getArgValue('--workspaceId');
  const dryRun = hasFlag('--dry-run');

  const result = await client.mutation(backfillMetadataRef, {
    workspaceId,
    dryRun,
  });

  console.log(JSON.stringify(result, null, 2));
}

main().catch((error) => {
  console.error('[planner backfill] failed');
  console.error(error);
  process.exitCode = 1;
});