import { ConvexHttpClient } from 'convex/browser';
import { makeFunctionReference } from 'convex/server';

const CONVEX_URL =
  process.env.CONVEX_SELF_HOSTED_URL ||
  process.env.CONVEX_URL ||
  'http://convex-backend:3210';

const getDocumentRef = makeFunctionReference('query', 'plannerDocuments:getDocument');

let client = null;

function getClient() {
  if (!client) {
    client = new ConvexHttpClient(CONVEX_URL, {
      skipConvexDeploymentUrlCheck: true,
      logger: false,
    });
  }

  return client;
}

export async function getPlannerDocument(workspaceId, documentId) {
  return await getClient().query(getDocumentRef, {
    workspaceId,
    documentId,
  });
}
