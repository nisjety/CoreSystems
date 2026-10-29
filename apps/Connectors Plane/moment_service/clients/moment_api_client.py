"""
Moment API client for time tracking and project management.

Implements comprehensive access to Moment API v1.0.

Based on Moment API documentation:
- Basic Auth using "apikey" as username and API key as password
- Rate limit: 120 req/sec per IP, 30 req/sec per token
- Company-specific endpoints

References:
- https://app.moment.team/api/1.0/docs/
"""

import base64
import logging
import os
import re
import time
from datetime import datetime
from typing import Any, Dict, List, Optional, Sequence
from urllib.parse import urlencode

import requests

from config import config

logger = logging.getLogger(__name__)


class MomentAPIError(Exception):
    """Base exception for Moment API errors."""
    pass


class MomentRateLimitError(MomentAPIError):
    """Raised when rate limit is exceeded (HTTP 429)."""
    pass


class MomentAuthenticationError(MomentAPIError):
    """Raised when authentication fails."""
    pass


class MomentTransportError(MomentAPIError):
    """
    Raised when the request never produced an HTTP response (connection reset,
    DNS failure, client-side timeout).

    Distinct from MomentAPIError so callers can tell "the server said no" from
    "we never heard back" - get_all_pages must not treat a network blip as the
    server rejecting a query parameter.
    """
    pass


# Moment cancels slow queries server-side at ~30s with a 400
# QueryTimeoutException. Give the client enough headroom to actually receive
# that response instead of racing it with its own timeout - a real 400 is far
# more actionable than "Read timed out".
DEFAULT_TIMEOUT = (10, 45)  # (connect, read) seconds
TRANSPORT_RETRIES = 2       # additional attempts, GET only

# Ascending sort keys tried, in order, to keep pagination stable. See
# get_all_pages for why more than one is needed.
STABLE_SORT_CANDIDATES = ("id,asc", "createdDate,asc")


def _brief(error: Exception, limit: int = 90) -> str:
    """Collapse an API error to one short line for log messages."""
    text = " ".join(str(error).split())
    return text if len(text) <= limit else f"{text[:limit]}..."


class MomentAPIClient:
    """
    Client for Moment API.

    Supports:
    - All Moment API endpoints (projects, time records, reports, etc.)
    - Basic Auth with API key
    - Rate limiting (120 req/sec per IP, 30 req/sec per token)
    - Automatic pagination
    - Batch operations (create, update, delete)
    - Includes concept for selective field retrieval
    """

    def __init__(
        self,
        api_key: Optional[str] = None,
        company_code: Optional[str] = None,
        base_url: Optional[str] = None,
    ):
        """
        Initialize Moment API client.

        Args:
            api_key: Moment API key (defaults to config.api_key)
            company_code: Company code in Moment (defaults to config.company_code)
            base_url: Base API URL (defaults to config.base_url)
        """
        self.api_key = api_key or config.api_key
        self.company_code = company_code or config.company_code
        self.base_url = base_url or config.base_url
        self.session = requests.Session()
        read_timeout = os.getenv("MOMENT_HTTP_READ_TIMEOUT_SECONDS")
        self.timeout = (DEFAULT_TIMEOUT[0], int(read_timeout)) if read_timeout else DEFAULT_TIMEOUT
        
        if not self.api_key:
            raise ValueError("API key is required")
        if not self.company_code:
            raise ValueError("Company code is required")
        
        # Set up Basic Auth header
        # Username is always "apikey", password is the API key
        auth_string = f"apikey:{self.api_key}"
        auth_bytes = auth_string.encode("utf-8")
        auth_b64 = base64.b64encode(auth_bytes).decode("utf-8")
        
        self.session.headers.update({
            "Authorization": f"Basic {auth_b64}",
            "Accept": "application/json",
            "Content-Type": "application/json",
        })
        
        logger.info(f"Initialized MomentAPIClient for company: {self.company_code}")

    def _get_endpoint_url(self, endpoint: str) -> str:
        """
        Build full endpoint URL.

        Args:
            endpoint: API endpoint path (e.g., "projects", "timeRecords")

        Returns:
            Full URL string
        """
        # Remove leading slash if present
        endpoint = endpoint.lstrip("/")
        
        return f"{self.base_url}/companies/{self.company_code}/{endpoint}"

    def _request(
        self,
        method: str,
        endpoint: str,
        params: Optional[Dict[str, Any]] = None,
        data: Optional[Dict[str, Any]] = None,
        json_data: Optional[Any] = None,
    ) -> Any:
        """
        Make authenticated request to Moment API.

        Args:
            method: HTTP method (GET, POST, PUT, PATCH, DELETE)
            endpoint: API endpoint path
            params: URL query parameters
            data: Request body as dict (for form data)
            json_data: Request body as JSON

        Returns:
            Response data (usually dict or list)

        Raises:
            MomentRateLimitError: When rate limit is exceeded
            MomentAuthenticationError: When authentication fails
            MomentAPIError: For other API errors
        """
        url = self._get_endpoint_url(endpoint)

        # Only GET is retried: it is the sole idempotent verb here, so a retry
        # after a timeout cannot create or mutate anything twice.
        attempts = TRANSPORT_RETRIES + 1 if method.upper() == "GET" else 1

        for attempt in range(1, attempts + 1):
            try:
                return self._attempt(method, url, params, data, json_data)
            except MomentTransportError as e:
                if attempt == attempts:
                    logger.error(f"Request failed after {attempt} attempt(s): {e}")
                    raise
                backoff = 2 ** (attempt - 1)
                logger.warning(f"{endpoint}: {e} - retrying in {backoff}s ({attempt}/{attempts - 1})")
                time.sleep(backoff)

    def _attempt(
        self,
        method: str,
        url: str,
        params: Optional[Dict[str, Any]] = None,
        data: Optional[Dict[str, Any]] = None,
        json_data: Optional[Any] = None,
    ) -> Any:
        """Single HTTP attempt. See _request for the retry/error contract."""
        try:
            response = self.session.request(
                method=method,
                url=url,
                params=params,
                data=data,
                json=json_data,
                timeout=self.timeout,
            )
            
            # Handle rate limiting
            if response.status_code == 429:
                retry_after = response.headers.get("Retry-After", "unknown")
                logger.warning(f"Rate limit exceeded. Retry after: {retry_after}")
                raise MomentRateLimitError(
                    f"Rate limit exceeded. Retry after: {retry_after}"
                )
            
            # Handle authentication errors
            if response.status_code == 401:
                logger.error("Authentication failed. Check API key.")
                raise MomentAuthenticationError("Authentication failed. Invalid API key.")
            
            # Handle other errors. Logged at WARNING, not ERROR: some 400s are
            # recoverable and self-corrected by the caller (get_all_pages
            # retries the known page-size cap), so logging ERROR here cries
            # wolf on every successful sync. Whoever ends up handling the
            # raised exception decides the real severity - sync_entity already
            # logs ERROR when a fetch actually fails.
            if response.status_code >= 400:
                error_msg = f"API request failed: {response.status_code} - {response.text}"
                logger.warning(error_msg)
                raise MomentAPIError(error_msg)
            
            # Parse JSON response
            if response.content:
                return response.json()
            return None
            
        except requests.exceptions.RequestException as e:
            # No HTTP response at all - never conflate this with the server
            # rejecting a parameter (see MomentTransportError).
            raise MomentTransportError(f"Request failed: {e}") from e

    # ===== CRUD Operations =====

    def get(
        self,
        endpoint: str,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """
        GET request to retrieve data.

        Args:
            endpoint: API endpoint path
            params: Query parameters

        Returns:
            Response data
        """
        return self._request("GET", endpoint, params=params)

    def get_by_id(
        self,
        endpoint: str,
        entity_id: int,
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        GET single entity by ID with optional includes.

        Args:
            endpoint: API endpoint path (e.g., "projects")
            entity_id: Entity ID
            includes: Optional includes string (e.g., "base+metadata")

        Returns:
            Entity data
        """
        url = f"{endpoint}/{entity_id}"
        if includes:
            url = f"{url}/include/{includes}"
        
        return self._request("GET", url)

    def get_list(
        self,
        endpoint: str,
        includes: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """
        GET list of entities with optional includes.

        Args:
            endpoint: API endpoint path
            includes: Optional includes string (e.g., "base+currencyCode+metadata.overviewPage")
            params: Additional query parameters

        Returns:
            List of entities or response with content/metadata
        """
        url = endpoint
        if includes:
            url = f"{url}/include/{includes}"

        return self._request("GET", url, params=params)

    def get_all_pages(
        self,
        endpoint: str,
        includes: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
        page_size: int = 200,
        sort_candidates: Optional[Sequence[str]] = None,
    ) -> List[Dict[str, Any]]:
        """
        Fetch every page of a paginated list endpoint and return the combined content.

        Moment paginates with Spring Data's page/size convention: the response
        is `{content: [...], page: {number, size, totalElements, totalPages}, ...}`.
        A single get_list() call only returns one page (20 items by default),
        which is not "all data" for any entity with more than a page of rows.

        First-page requests self-correct on two server-side limits instead of
        failing the whole fetch:

        - Page size: some endpoints (observed on the bare/full "projects",
          "customers" and "invoices" representations, which embed much heavier
          per-row payloads than their "include/base" counterparts) reject a page
          size above 100 with a 400 `"Max size for this endpoint is 100"`. The
          cap is read out of the message and the request retried at that size.
        - Sort key: the injected stable sort is retried down STABLE_SORT_CANDIDATES
          and only then dropped (with a warning - see the sort comment below).

        Transport failures are retried inside _request and are never mistaken
        for either of the above.

        Args:
            endpoint: API endpoint path
            includes: Optional includes string
            params: Additional query/filter parameters (page/size are managed here)
            page_size: Page size to request per call (Moment allows up to 999
                on most endpoints, but as low as 100 on some - see above)
            sort_candidates: Override the order of stable sort keys to try.
                Use when an endpoint is known to choke on the default first
                choice, to skip straight past a probe that always fails - the
                remaining candidates still act as the fallback chain.

        Returns:
            Combined list of every item across all pages
        """
        all_items: List[Dict[str, Any]] = []
        page_number = 0
        base_params = dict(params or {})
        base_params.setdefault("size", page_size)
        size = base_params["size"]

        # Default to a stable ascending sort key: Moment assigns monotonically
        # increasing ids and createdDate, so an ascending sort keeps existing
        # rows at fixed page positions while data changes mid-fetch - records
        # created during the crawl land on the final pages instead of shifting
        # earlier pages (which would duplicate some rows and skip others).
        # Caller-provided sort wins.
        #
        # Candidates are tried in order because the sort is not merely accepted
        # or rejected: on timeRecords, `id,asc` parses fine but the server
        # cancels the page-0 query at ~30s with a 400 QueryTimeoutException
        # (reproducible at every page size), while `createdDate,asc` answers the
        # same page in about a second. Falling straight to unsorted would strip
        # the stability guarantee from the single largest entity.
        candidates = list(sort_candidates or STABLE_SORT_CANDIDATES) if "sort" not in base_params else []
        if candidates:
            base_params["sort"] = candidates[0]

        total_elements_first = None

        while True:
            # First-page requests self-correct on the known parameter
            # rejections (page-size cap, unusable sort key); anything else, or
            # an error past page 0, propagates. Transport failures are retried
            # inside _request and never degrade the sort.
            while True:
                try:
                    response = self.get_list(endpoint, includes=includes, params={**base_params, "page": page_number})
                    break
                except (MomentTransportError, MomentRateLimitError):
                    # Neither says anything about the query parameters, so
                    # neither may trigger the page-size or sort fallbacks.
                    raise
                except MomentAPIError as e:
                    if page_number != 0:
                        raise
                    match = re.search(r"[Mm]ax size for this endpoint is (\d+)", str(e))
                    if match and base_params["size"] != int(match.group(1)):
                        size = int(match.group(1))
                        base_params["size"] = size
                        logger.info(f"{endpoint}: server rejected page size, retrying with size={size}")
                        continue
                    if candidates:
                        rejected = candidates.pop(0)
                        if candidates:
                            base_params["sort"] = candidates[0]
                            logger.info(
                                f"{endpoint}: sort={rejected} unusable ({_brief(e)}), "
                                f"retrying with sort={candidates[0]}"
                            )
                        else:
                            base_params.pop("sort", None)
                            logger.warning(
                                f"{endpoint}: no usable stable sort key (last tried {rejected}: "
                                f"{_brief(e)}) - paging UNSORTED, so rows created or edited "
                                f"mid-crawl may shift between pages and be missed"
                            )
                        continue
                    raise

            if isinstance(response, dict) and "content" in response:
                items = response["content"]
                all_items.extend(items)
                page_meta = response.get("page") or {}
                total_elements = page_meta.get("totalElements")
                if total_elements_first is None:
                    total_elements_first = total_elements
                elif (
                    not base_params.get("sort")
                    and total_elements is not None
                    and total_elements != total_elements_first
                ):
                    # Only meaningful without a stable sort: with one, new rows
                    # land past the crawl and change nothing already read.
                    logger.warning(
                        f"{endpoint}: row count moved during an unsorted crawl "
                        f"({total_elements_first} -> {total_elements}); duplicates are "
                        f"deduped downstream, but some rows may have been skipped"
                    )
                    total_elements_first = total_elements
                total_pages = page_meta.get("totalPages")
                if total_pages is not None:
                    if page_number >= total_pages - 1:
                        break
                elif len(items) < size:
                    break
            elif isinstance(response, list):
                all_items.extend(response)
                if len(response) < size:
                    break
            else:
                break

            page_number += 1

        return all_items

    def create(
        self,
        endpoint: str,
        data: Dict[str, Any],
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        POST to create new entity.

        Args:
            endpoint: API endpoint path
            data: Entity data
            includes: Optional includes for response

        Returns:
            Created entity data
        """
        url = endpoint
        if includes:
            url = f"{url}/include/{includes}"
        
        return self._request("POST", url, json_data=data)

    def update(
        self,
        endpoint: str,
        entity_id: int,
        data: Dict[str, Any],
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        PUT/PATCH to update entity.

        Args:
            endpoint: API endpoint path
            entity_id: Entity ID
            data: Updated data (can be partial)
            includes: Optional includes for response

        Returns:
            Updated entity data
        """
        url = f"{endpoint}/{entity_id}"
        if includes:
            url = f"{url}/include/{includes}"
        
        return self._request("PATCH", url, json_data=data)

    def delete(
        self,
        endpoint: str,
        entity_id: int,
    ) -> None:
        """
        DELETE entity.

        Args:
            endpoint: API endpoint path
            entity_id: Entity ID
        """
        url = f"{endpoint}/{entity_id}"
        self._request("DELETE", url)

    # ===== Batch Operations =====

    def batch(
        self,
        endpoint: str,
        create: Optional[List[Dict[str, Any]]] = None,
        update: Optional[Dict[int, Dict[str, Any]]] = None,
        delete: Optional[List[int]] = None,
    ) -> Dict[str, Any]:
        """
        Batch create, update, and delete operations.

        Args:
            endpoint: API endpoint path
            create: List of entities to create
            update: Dict of {id: data} for entities to update
            delete: List of IDs to delete

        Returns:
            Batch operation results with created/updated entities

        Example:
            client.batch(
                "timeRecords",
                create=[{"projectId": 123, "hours": 8}],
                update={456: {"hours": 6}},
                delete=[789]
            )
        """
        batch_data = {}
        
        if create:
            batch_data["create"] = create
        if update:
            batch_data["update"] = update
        if delete:
            batch_data["delete"] = delete
        
        return self._request("POST", f"{endpoint}/batch", json_data=batch_data)

    # ===== Projects =====

    def get_projects(
        self,
        includes: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """
        Get list of projects.

        Args:
            includes: Optional includes (e.g., "base+customer")
            params: Additional filters

        Returns:
            List of projects
        """
        return self.get_list("projects", includes=includes, params=params)

    def get_project(
        self,
        project_id: int,
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Get single project by ID.

        Args:
            project_id: Project ID
            includes: Optional includes

        Returns:
            Project data
        """
        return self.get_by_id("projects", project_id, includes=includes)

    def get_project_registered_hours_report(
        self,
        from_date: str,
        to_date: str,
        **kwargs,
    ) -> Any:
        """
        Get project registered hours report.

        Args:
            from_date: Start date (YYYY-MM-DD)
            to_date: End date (YYYY-MM-DD)
            **kwargs: Additional parameters (projectTagOperator, billableRateOption, etc.)

        Returns:
            Report data

        Example:
            report = client.get_project_registered_hours_report(
                from_date="2024-01-01",
                to_date="2024-01-31",
                billableRateOption="registered",
                subtractAbsences="countAsBillable"
            )
        """
        params = {
            "fromDate": from_date,
            "toDate": to_date,
            **kwargs,
        }
        return self.get("projects/projectRegisteredHoursReport", params=params)

    # ===== Time Records =====

    def get_time_records(
        self,
        includes: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
        from_date: Optional[str] = None,
        to_date: Optional[str] = None,
    ) -> Any:
        """
        Get list of time records.

        Note: the timeRecords list filter uses "from"/"to" (TimeRecordFilter),
        not "fromDate"/"toDate" — those are only accepted by the report
        endpoints (get_project_registered_hours_report, get_time_records_report).

        Args:
            includes: Optional includes
            params: Additional filters
            from_date: Optional start date (YYYY-MM-DD), maps to the "from" filter
            to_date: Optional end date (YYYY-MM-DD), maps to the "to" filter

        Returns:
            List of time records
        """
        query_params = dict(params or {})
        if from_date:
            query_params["from"] = from_date
        if to_date:
            query_params["to"] = to_date
        return self.get_list("timeRecords", includes=includes, params=query_params or None)

    def get_time_record(
        self,
        record_id: int,
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Get single time record by ID.

        Args:
            record_id: Time record ID
            includes: Optional includes

        Returns:
            Time record data
        """
        return self.get_by_id("timeRecords", record_id, includes=includes)

    def create_time_record(
        self,
        data: Dict[str, Any],
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Create new time record.

        Args:
            data: Time record data (projectId, date, hours, etc.)
            includes: Optional includes for response

        Returns:
            Created time record
        """
        return self.create("timeRecords", data, includes=includes)

    def update_time_record(
        self,
        record_id: int,
        data: Dict[str, Any],
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Update time record.

        Args:
            record_id: Time record ID
            data: Updated data
            includes: Optional includes

        Returns:
            Updated time record
        """
        return self.update("timeRecords", record_id, data, includes=includes)

    def delete_time_record(
        self,
        record_id: int,
    ) -> None:
        """
        Delete time record.

        Args:
            record_id: Time record ID
        """
        self.delete("timeRecords", record_id)

    def get_time_records_report(
        self,
        from_date: str,
        group_by: Optional[str] = None,
        **kwargs,
    ) -> Any:
        """
        Get complex time records report.

        Args:
            from_date: Start date (YYYY-MM-DD)
            group_by: Grouping (e.g., "userAccount,week")
            **kwargs: Additional parameters

        Returns:
            Report data

        Example:
            report = client.get_time_records_report(
                from_date="2024-01-01",
                group_by="userAccount,week",
                projectId=123,
                showProjectResourceReservationInfo=True
            )
        """
        params = {
            "from": from_date,
            **kwargs,
        }
        if group_by:
            params["groupBy"] = group_by
        
        return self.get("timeRecords/getReport", params=params)

    # ===== Customers =====

    def get_customers(
        self,
        includes: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Get list of customers."""
        return self.get_list("customers", includes=includes, params=params)

    def get_customer(
        self,
        customer_id: int,
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Get single customer by ID."""
        return self.get_by_id("customers", customer_id, includes=includes)

    # ===== User Accounts =====

    def get_user_accounts(
        self,
        includes: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Get list of user accounts."""
        return self.get_list("userAccounts", includes=includes, params=params)

    def get_user_account(
        self,
        user_id: int,
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Get single user account by ID."""
        return self.get_by_id("userAccounts", user_id, includes=includes)

    # ===== Tags =====

    def get_tags(
        self,
        includes: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Get list of tags."""
        return self.get_list("tags", includes=includes, params=params)

    # ===== Invoices =====

    def get_invoices(
        self,
        includes: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Get list of invoices."""
        return self.get_list("invoices", includes=includes, params=params)

    def get_invoice(
        self,
        invoice_id: int,
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Get single invoice by ID."""
        return self.get_by_id("invoices", invoice_id, includes=includes)

    # ===== Offers =====

    def get_offers(
        self,
        includes: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Get list of offers (quotes/proposals)."""
        return self.get_list("offers", includes=includes, params=params)

    def get_offer(
        self,
        offer_id: int,
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """Get single offer by ID."""
        return self.get_by_id("offers", offer_id, includes=includes)

    # ===== Invoicing Currency Setups =====

    def get_invoicing_currency_setups(
        self,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Get list of invoicing currency setups."""
        return self.get_list("invoicingCurrencySetups", params=params)

    # ===== Project Membership Roles =====

    def get_project_membership_roles(
        self,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Get list of project membership roles."""
        return self.get_list("projectMembershipRoles", params=params)

    # ===== Expense Categories =====

    def get_expense_categories(
        self,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Get list of expense categories."""
        return self.get_list("expenseCategories", params=params)

    # ===== Custom Fields =====

    def get_custom_fields(
        self,
        custom_field_entity: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """
        Get list of custom field definitions.

        Args:
            custom_field_entity: Optional filter, e.g. "project", "customer", "task"
            params: Additional filters
        """
        query_params = dict(params or {})
        if custom_field_entity:
            query_params["customFieldEntity"] = custom_field_entity
        return self.get_list("customFields", params=query_params or None)

    # ===== Departments =====

    def get_departments(
        self,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Get list of departments."""
        return self.get_list("departments", params=params)

    # ===== Absences =====

    def get_absences(
        self,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Get list of absences."""
        return self.get_list("absences", params=params)

    # ===== Generic Methods =====

    def get_entities(
        self,
        entity_type: str,
        includes: Optional[str] = None,
        params: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """
        Generic method to get any entity type.

        Args:
            entity_type: Entity endpoint (e.g., "projects", "timeRecords", "customers")
            includes: Optional includes
            params: Additional filters

        Returns:
            Entity data
        """
        return self.get_list(entity_type, includes=includes, params=params)

    def get_entity(
        self,
        entity_type: str,
        entity_id: int,
        includes: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Generic method to get single entity.

        Args:
            entity_type: Entity endpoint
            entity_id: Entity ID
            includes: Optional includes

        Returns:
            Entity data
        """
        return self.get_by_id(entity_type, entity_id, includes=includes)
