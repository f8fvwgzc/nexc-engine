---
id: cloud-landing-zone-and-networking
name: Landing zones and networking
category: cloud
description: Designs a cloud landing zone across AWS, Azure and Google Cloud: account, subscription or project hierarchy, identity foundation, network topology and IP plan, hybrid links and preventive guardrails.
keywords: landing zone, multi-account, organizational units, management groups, resource hierarchy, control tower, vpc, vnet, hub and spoke, transit gateway, ip address plan, cidr, scp, azure policy, organization policy, hybrid connectivity
source: AWS - Organizing your AWS environment using multiple accounts | https://docs.aws.amazon.com/whitepapers/latest/organizing-your-aws-environment/organizing-your-aws-environment.html
source: AWS Organizations - Resource control policies | https://docs.aws.amazon.com/organizations/latest/userguide/orgs_manage_policies_rcps.html
source: Microsoft Cloud Adoption Framework - Management groups | https://learn.microsoft.com/en-us/azure/cloud-adoption-framework/ready/landing-zone/design-area/resource-org-management-groups
source: Microsoft Cloud Adoption Framework - Plan for IP addressing | https://learn.microsoft.com/en-us/azure/cloud-adoption-framework/ready/azure-best-practices/plan-for-ip-addressing
source: Google Cloud - Decide a resource hierarchy for your landing zone | https://docs.cloud.google.com/architecture/landing-zones/decide-resource-hierarchy
---
## When to use
New cloud foundation, growth past a few accounts, a merger, or fixing an estate that grew unplanned. These choices are hard to reverse: settle them first.

## Hierarchy
- AWS: Organization with all features. Management account = billing and org admin only, no workloads. Security OU for the log archive and security tooling accounts (no workloads); Infrastructure OU for network and shared services; add Workloads (Prod/SDLC), Sandbox, Policy Staging, Suspended, Exceptions as needed. One account per workload per environment; nothing crosses accounts unless granted.
- Azure: tenant root > intermediate root > Platform (Security, Management, Connectivity, Identity) and Landing zones (Corp, Online, Local for Azure Local), plus Sandboxes and Decommissioned. Keep 3-4 levels (hard limit six below the root). Environments are subscriptions, not management groups; no per-region groups unless residency law requires. Set a default management group (e.g. Sandboxes) for new subscriptions; grant workload RBAC at subscription or resource group; platform access through PIM.
- Google Cloud: one organization. Top folders by region/subsidiary when policies differ strongly, else by product team when teams need autonomy, else by environment; hybrids are normal. Bootstrap folder under the org for IaC seed projects; billing binds to projects, not folders.
- Vend accounts/subscriptions/projects by automation (Control Tower Account Factory, subscription vending, project factory) with logging, budget, baseline policy and network attachment applied at creation.

## Identity foundation
One workforce IdP federated to AWS IAM Identity Center, Azure RBAC (Entra ID) and Google Cloud IAM; groups map to permission sets or roles; no IAM users with long-lived keys; two monitored break-glass accounts with phishing-resistant MFA; workloads use roles, managed identities or workload identity federation.

## Guardrails
- AWS SCPs cap what member-account principals may do; RCPs cap access to member-account resources, including by outside principals. Neither grants anything; neither affects the management account.
- Azure Policy at management group scope (deny, audit, modify, deployIfNotExists). GCP organization policy constraints (gcp.resourceLocations, iam.disableServiceAccountKeyCreation, compute.vmExternalIpAccess, iam.allowedPolicyMemberDomains), inherited with controlled overrides; IAM deny policies are evaluated before allow.
- Baseline: region allow-list, protect logging and security services, block leaving the org, deny public storage, require encryption and tags.
- Test on a staging OU or management group, then promote; read denial logs. Detective layer: config rules plus Security Hub, Defender for Cloud or Security Command Center; central immutable log archive.

## Network and IP plan
- Hub-and-spoke: AWS Transit Gateway or Cloud WAN; Azure hub VNet or Virtual WAN; GCP Shared VPC per environment or Network Connectivity Center. Centralise egress inspection, DNS and hybrid links in the hub.
- IP plan in an IPAM: non-overlapping RFC 1918 blocks per region and environment, never overlapping on-premises; spoke sizes /24, /22, /20; avoid blanket /16 VNets; AWS and Azure reserve 5 addresses per subnet, Google Cloud 4 per primary range; reserve Kubernetes pod and service ranges; IPv6 dual stack where IPv4 is scarce.
- Hybrid: Direct Connect, ExpressRoute or Cloud Interconnect in pairs at two sites, IPsec VPN as backup, BGP with route filters and summaries.
- PaaS reached privately (PrivateLink, Private Endpoint, Private Service Connect) with private DNS zones in the hub.

## Pitfalls
Workloads in the management account; one flat network for all environments; CIDR overlap found at peering time; untested SCPs attached at root; guardrails not kept in code.
