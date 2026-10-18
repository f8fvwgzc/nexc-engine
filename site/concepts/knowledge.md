# Knowledge

This page explains how uploaded documents become passages an agent can read, how topics are found, and how classification limits what each agent sees.

## Documents

A member uploads a file to a workspace (`POST /workspaces/{wid}/documents`, at most 50 MiB). The server parses it, splits it into passages and embeds them; the document's status moves from `pending` through `parsing` and `embedding` to `ready`, or to `failed` with the error. Guests do not see documents. Whoever uploaded a document, or an admin, can delete it.

## Topics

Topics are found without supervision: passages are clustered by their embeddings and each cluster is named by the words that set it apart. They appear once a workspace has about twenty passages, and new passages join the nearest topic. `POST /workspaces/{wid}/knowledge/topics/rebuild` computes them again.

## Classification

A document and a topic carry a data label: `public`, `internal`, `confidential` or `restricted`. A workspace admin classifies either one (`PATCH /workspaces/{wid}/documents/{did}`, `PATCH /workspaces/{wid}/knowledge/topics/{tid}`); every passage under a topic takes the stricter of its document's label and the topic's, and the change is audited as `classification_changed`.

An agent is given only the passages its clearance and its topic scope allow (`data.labels` and `data.topics` of its capabilities), and what the context builder read for a node is recorded as a `data_read` span before the node runs. See [Identity](/secops/identity) and [Policy and defend](/secops/policy).

## Settings and search

**Settings, Knowledge** holds a workspace's knowledge settings (`GET` and `PUT /workspaces/{wid}/knowledge/settings`): the embedding endpoint, model and dimensions, how many passages and how many characters go into a prompt, and whether knowledge is used in nodes and in plans. `GET /workspaces/{wid}/knowledge/search?q=&topic_id=&limit=` searches the passages. The endpoints are listed in the [endpoints](/reference/endpoints).
