"""SuperOffice CRM Agent API Client.

Provides access to SuperOffice Agent API methods for advanced operations.
Based on Swagger-v1-Agents.json specification.

The Agent API provides service-oriented methods for complex business logic
that go beyond simple CRUD operations.
"""

import logging
from typing import Dict, List, Optional, Any
from datetime import datetime

from clients.socrm_client import SuperOfficeClient, SuperOfficeAPIError

logger = logging.getLogger(__name__)


class SuperOfficeAgentClient(SuperOfficeClient):
    """Extended client for SuperOffice Agent API operations."""
    
    # Contact Agent Methods
    def create_default_contact_entity(self) -> Dict[str, Any]:
        """Create a new ContactEntity with default values."""
        response = self._make_request(
            "POST",
            "/Agents/Contact/CreateDefaultContactEntity"
        )
        return response
    
    def get_contact_entity(self, contact_id: int) -> Dict[str, Any]:
        """Get a ContactEntity object (Agent API version with full entity data)."""
        response = self._make_request(
            "POST",
            "/Agents/Contact/GetContactEntity",
            json={"contactId": contact_id}
        )
        return response
    
    def save_contact_entity(self, contact_entity: Dict[str, Any]) -> Dict[str, Any]:
        """Save a ContactEntity (create or update)."""
        response = self._make_request(
            "POST",
            "/Agents/Contact/SaveContactEntity",
            json={"contactEntity": contact_entity}
        )
        return response
    
    def delete_contact_entity(self, contact_id: int) -> None:
        """Delete a ContactEntity."""
        self._make_request(
            "POST",
            "/Agents/Contact/DeleteContactEntity",
            json={"contactId": contact_id}
        )
    
    def get_contact_with_persons(self, contact_id: int) -> Dict[str, Any]:
        """Get a ContactEntity with all associated persons."""
        response = self._make_request(
            "POST",
            "/Agents/Contact/GetContactWithPersons",
            json={"contactId": contact_id}
        )
        return response
    
    # Person Agent Methods
    def create_default_person_entity(self) -> Dict[str, Any]:
        """Create a new PersonEntity with default values."""
        response = self._make_request(
            "POST",
            "/Agents/Person/CreateDefaultPersonEntity"
        )
        return response
    
    def get_person_entity(self, person_id: int) -> Dict[str, Any]:
        """Get a PersonEntity object."""
        response = self._make_request(
            "POST",
            "/Agents/Person/GetPersonEntity",
            json={"personId": person_id}
        )
        return response
    
    def save_person_entity(self, person_entity: Dict[str, Any]) -> Dict[str, Any]:
        """Save a PersonEntity (create or update)."""
        response = self._make_request(
            "POST",
            "/Agents/Person/SavePersonEntity",
            json={"personEntity": person_entity}
        )
        return response
    
    def delete_person_entity(self, person_id: int) -> None:
        """Delete a PersonEntity."""
        self._make_request(
            "POST",
            "/Agents/Person/DeletePersonEntity",
            json={"personId": person_id}
        )
    
    def get_person_list(self, person_ids: List[int]) -> List[Dict[str, Any]]:
        """Get multiple persons by their IDs."""
        response = self._make_request(
            "POST",
            "/Agents/Person/GetPersonList",
            json={"personIds": person_ids}
        )
        return response
    
    # Sale Agent Methods
    def create_default_sale_entity(self) -> Dict[str, Any]:
        """Create a new SaleEntity with default values."""
        response = self._make_request(
            "POST",
            "/Agents/Sale/CreateDefaultSaleEntity"
        )
        return response
    
    def get_sale_entity(self, sale_id: int) -> Dict[str, Any]:
        """Get a SaleEntity object."""
        response = self._make_request(
            "POST",
            "/Agents/Sale/GetSaleEntity",
            json={"saleId": sale_id}
        )
        return response
    
    def save_sale_entity(self, sale_entity: Dict[str, Any]) -> Dict[str, Any]:
        """Save a SaleEntity (create or update)."""
        response = self._make_request(
            "POST",
            "/Agents/Sale/SaveSaleEntity",
            json={"saleEntity": sale_entity}
        )
        return response
    
    def delete_sale_entity(self, sale_id: int) -> None:
        """Delete a SaleEntity."""
        self._make_request(
            "POST",
            "/Agents/Sale/DeleteSaleEntity",
            json={"saleId": sale_id}
        )
    
    def set_as_lost(self, sale_id: int) -> Dict[str, Any]:
        """Mark a sale as lost."""
        response = self._make_request(
            "POST",
            "/Agents/Sale/SetAsLost",
            json={"saleId": sale_id}
        )
        return response
    
    def set_as_sold(self, sale_id: int) -> Dict[str, Any]:
        """Mark a sale as sold."""
        response = self._make_request(
            "POST",
            "/Agents/Sale/SetAsSold",
            json={"saleId": sale_id}
        )
        return response
    
    def get_sale_stakeholders(self, sale_id: int) -> List[Dict[str, Any]]:
        """Get all stakeholders for a sale."""
        response = self._make_request(
            "POST",
            "/Agents/Sale/GetSaleStakeholders",
            json={"saleId": sale_id}
        )
        return response
    
    # Project Agent Methods
    def create_default_project_entity(self) -> Dict[str, Any]:
        """Create a new ProjectEntity with default values."""
        response = self._make_request(
            "POST",
            "/Agents/Project/CreateDefaultProjectEntity"
        )
        return response
    
    def get_project_entity(self, project_id: int) -> Dict[str, Any]:
        """Get a ProjectEntity object."""
        response = self._make_request(
            "POST",
            "/Agents/Project/GetProjectEntity",
            json={"projectId": project_id}
        )
        return response
    
    def save_project_entity(self, project_entity: Dict[str, Any]) -> Dict[str, Any]:
        """Save a ProjectEntity (create or update)."""
        response = self._make_request(
            "POST",
            "/Agents/Project/SaveProjectEntity",
            json={"projectEntity": project_entity}
        )
        return response
    
    def delete_project_entity(self, project_id: int) -> None:
        """Delete a ProjectEntity."""
        self._make_request(
            "POST",
            "/Agents/Project/DeleteProjectEntity",
            json={"projectId": project_id}
        )
    
    def get_project_members(self, project_id: int) -> List[Dict[str, Any]]:
        """Get all members of a project."""
        response = self._make_request(
            "POST",
            "/Agents/Project/GetProjectMembers",
            json={"projectId": project_id}
        )
        return response
    
    # Appointment Agent Methods
    def create_default_appointment_entity(self) -> Dict[str, Any]:
        """Create a new AppointmentEntity with default values."""
        response = self._make_request(
            "POST",
            "/Agents/Appointment/CreateDefaultAppointmentEntity"
        )
        return response
    
    def get_appointment_entity(self, appointment_id: int) -> Dict[str, Any]:
        """Get an AppointmentEntity object."""
        response = self._make_request(
            "POST",
            "/Agents/Appointment/GetAppointmentEntity",
            json={"appointmentId": appointment_id}
        )
        return response
    
    def save_appointment_entity(
        self,
        appointment_entity: Dict[str, Any]
    ) -> Dict[str, Any]:
        """Save an AppointmentEntity (create or update)."""
        response = self._make_request(
            "POST",
            "/Agents/Appointment/SaveAppointmentEntity",
            json={"appointmentEntity": appointment_entity}
        )
        return response
    
    def delete_appointment_entity(self, appointment_id: int) -> None:
        """Delete an AppointmentEntity."""
        self._make_request(
            "POST",
            "/Agents/Appointment/DeleteAppointmentEntity",
            json={"appointmentId": appointment_id}
        )
    
    def accept_appointment(
        self,
        appointment_id: int,
        update_mode: int = 0
    ) -> Dict[str, Any]:
        """Accept an appointment invitation."""
        response = self._make_request(
            "POST",
            "/Agents/Appointment/Accept",
            json={
                "appointmentId": appointment_id,
                "updateMode": update_mode
            }
        )
        return response
    
    def decline_appointment(
        self,
        appointment_id: int,
        update_mode: int = 0
    ) -> Dict[str, Any]:
        """Decline an appointment invitation."""
        response = self._make_request(
            "POST",
            "/Agents/Appointment/Decline",
            json={
                "appointmentId": appointment_id,
                "updateMode": update_mode
            }
        )
        return response
    
    # Document Agent Methods
    def create_default_document_entity(self) -> Dict[str, Any]:
        """Create a new DocumentEntity with default values."""
        response = self._make_request(
            "POST",
            "/Agents/Document/CreateDefaultDocumentEntity"
        )
        return response
    
    def get_document_entity(self, document_id: int) -> Dict[str, Any]:
        """Get a DocumentEntity object."""
        response = self._make_request(
            "POST",
            "/Agents/Document/GetDocumentEntity",
            json={"documentId": document_id}
        )
        return response
    
    def save_document_entity(
        self,
        document_entity: Dict[str, Any]
    ) -> Dict[str, Any]:
        """Save a DocumentEntity (create or update)."""
        response = self._make_request(
            "POST",
            "/Agents/Document/SaveDocumentEntity",
            json={"documentEntity": document_entity}
        )
        return response
    
    def delete_document_entity(self, document_id: int) -> None:
        """Delete a DocumentEntity."""
        self._make_request(
            "POST",
            "/Agents/Document/DeleteDocumentEntity",
            json={"documentId": document_id}
        )
    
    def get_document_stream(self, document_id: int) -> bytes:
        """Get document content as binary stream."""
        response = self._make_request(
            "POST",
            "/Agents/Document/GetDocumentStream",
            json={"documentId": document_id}
        )
        # Note: In reality, this would need special handling for binary data
        return response
    
    # Archive Agent Methods
    def get_archive_list_by_columns(
        self,
        provider_name: str,
        columns: List[str],
        sort_order: Optional[str] = None,
        restriction: Optional[str] = None,
        entities: Optional[str] = None,
        page: int = 0,
        page_size: int = 100
    ) -> Dict[str, Any]:
        """Get archive data using the Archive Agent.
        
        This is the most flexible way to query data in SuperOffice.
        """
        request_data = {
            "providerName": provider_name,
            "columns": columns,
            "sortOrder": sort_order or "",
            "restriction": restriction or "",
            "entities": entities or "",
            "page": page,
            "pageSize": page_size
        }
        
        response = self._make_request(
            "POST",
            "/Agents/Archive/GetArchiveListByColumns",
            json=request_data
        )
        return response
    
    # List Agent Methods
    def get_all_from_list_name(self, list_name: str) -> List[Dict[str, Any]]:
        """Get all items from a list by name."""
        response = self._make_request(
            "POST",
            "/Agents/List/GetAllFromListName",
            json={"udListDefinitionName": list_name}
        )
        return response
    
    def get_list_item(
        self,
        list_item_id: int,
        list_name: str
    ) -> Dict[str, Any]:
        """Get a specific list item."""
        response = self._make_request(
            "POST",
            "/Agents/List/GetListItem",
            json={
                "listItemId": list_item_id,
                "udListDefinitionName": list_name
            }
        )
        return response
    
    # Relation Agent Methods
    def create_default_contact_relation_entity(self) -> Dict[str, Any]:
        """Create a new ContactRelationEntity with default values."""
        response = self._make_request(
            "POST",
            "/Agents/Relation/CreateDefaultContactRelationEntity"
        )
        return response
    
    def save_contact_relation(
        self,
        relation: Dict[str, Any]
    ) -> Dict[str, Any]:
        """Save a contact relation (create or update)."""
        response = self._make_request(
            "POST",
            "/Agents/Relation/SaveContactRelation",
            json={"contactRelation": relation}
        )
        return response
    
    def delete_contact_relation(self, contact_relation_id: int) -> None:
        """Delete a contact relation."""
        self._make_request(
            "POST",
            "/Agents/Relation/DeleteContactRelation",
            json={"contactRelationId": contact_relation_id}
        )
    
    # Selection Agent Methods
    def create_default_selection_entity(self) -> Dict[str, Any]:
        """Create a new SelectionEntity with default values."""
        response = self._make_request(
            "POST",
            "/Agents/Selection/CreateDefaultSelectionEntity"
        )
        return response
    
    def get_selection_entity(self, selection_id: int) -> Dict[str, Any]:
        """Get a SelectionEntity object."""
        response = self._make_request(
            "POST",
            "/Agents/Selection/GetSelectionEntity",
            json={"selectionId": selection_id}
        )
        return response
    
    def save_selection_entity(
        self,
        selection_entity: Dict[str, Any]
    ) -> Dict[str, Any]:
        """Save a SelectionEntity (create or update)."""
        response = self._make_request(
            "POST",
            "/Agents/Selection/SaveSelectionEntity",
            json={"selectionEntity": selection_entity}
        )
        return response
    
    def add_contact_selection_members(
        self,
        selection_id: int,
        contact_ids: List[int]
    ) -> None:
        """Add contacts to a selection."""
        self._make_request(
            "POST",
            "/Agents/Selection/AddContactSelectionMembers",
            json={
                "selectionId": selection_id,
                "contactIds": contact_ids
            }
        )
    
    def remove_contact_selection_members(
        self,
        selection_id: int,
        contact_ids: List[int]
    ) -> None:
        """Remove contacts from a selection."""
        self._make_request(
            "POST",
            "/Agents/Selection/RemoveContactSelectionMembers",
            json={
                "selectionId": selection_id,
                "contactIds": contact_ids
            }
        )
    
    # User Agent Methods
    def get_user(self, associate_id: int) -> Dict[str, Any]:
        """Get user by associate ID."""
        response = self._make_request(
            "POST",
            "/Agents/User/GetUser",
            json={"associateId": associate_id}
        )
        return response
    
    def get_user_from_email(self, email: str) -> Dict[str, Any]:
        """Find user by email address."""
        response = self._make_request(
            "POST",
            "/Agents/User/GetUserFromEmail",
            json={"email": email}
        )
        return response
    
    def get_user_info_list(
        self,
        user_ids: List[int]
    ) -> List[Dict[str, Any]]:
        """Get information about multiple users."""
        response = self._make_request(
            "POST",
            "/Agents/User/GetUserInfoList",
            json={"userIds": user_ids}
        )
        return response
    
    # Find Agent Methods (Search)
    def find_contact(
        self,
        name: str = "",
        department: str = "",
        orgnr: str = "",
        phone: str = ""
    ) -> List[Dict[str, Any]]:
        """Search for contacts using multiple criteria."""
        response = self._make_request(
            "POST",
            "/Agents/Find/FindContact",
            json={
                "name": name,
                "department": department,
                "orgnr": orgnr,
                "phoneNumber": phone
            }
        )
        return response
    
    def find_person(
        self,
        firstname: str = "",
        lastname: str = "",
        phone: str = "",
        email: str = ""
    ) -> List[Dict[str, Any]]:
        """Search for persons using multiple criteria."""
        response = self._make_request(
            "POST",
            "/Agents/Find/FindPerson",
            json={
                "firstname": firstname,
                "lastname": lastname,
                "phoneNumber": phone,
                "email": email
            }
        )
        return response
    
    # Preference Agent Methods
    def get_preference(
        self,
        section: str,
        key: str
    ) -> Dict[str, Any]:
        """Get a preference value."""
        response = self._make_request(
            "POST",
            "/Agents/Preference/GetPreference",
            json={
                "section": section,
                "key": key
            }
        )
        return response
    
    def save_preference(
        self,
        section: str,
        key: str,
        value: str,
        level: str = "Database"
    ) -> None:
        """Save a preference value.
        
        Args:
            section: Preference section
            key: Preference key
            value: Value to set
            level: Preference level (Database, Group, Associate, etc.)
        """
        self._make_request(
            "POST",
            "/Agents/Preference/SavePreference",
            json={
                "section": section,
                "key": key,
                "value": value,
                "level": level
            }
        )
