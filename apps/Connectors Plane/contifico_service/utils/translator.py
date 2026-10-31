"""Translation utilities for converting Spanish field names to English."""
from typing import Dict, Any, List, Optional


class ContificoTranslator:
    """Translator for Contifico API Spanish-English field mappings."""

    # Field name mappings: Spanish -> English
    FIELD_TRANSLATIONS = {
        # Common fields
        "id": "id",
        "codigo": "code",
        "nombre": "name",
        "descripcion": "description",
        "fecha": "date",
        "estado": "status",
        "tipo": "type",
        "precio": "price",
        "cantidad": "quantity",
        "total": "total",
        "subtotal": "subtotal",
        "descuento": "discount",
        "impuesto": "tax",
        
        # Product/Inventory fields
        "producto": "product",
        "categoria": "category",
        "bodega": "warehouse",
        "variante": "variant",
        "marca": "brand",
        "stock": "stock",
        "costo": "cost",
        "precio_venta": "sale_price",
        "precio_compra": "purchase_price",
        "unidad_medida": "unit_of_measure",
        "existencia": "stock_quantity",
        
        # Document fields
        "documento": "document",
        "numero": "number",
        "serie": "series",
        "secuencial": "sequential",
        "autorizacion": "authorization",
        "clave_acceso": "access_key",
        "fecha_emision": "issue_date",
        "fecha_autorizacion": "authorization_date",
        "comprobante": "voucher",
        
        # Person/Contact fields
        "persona": "person",
        "cliente": "customer",
        "proveedor": "supplier",
        "razon_social": "business_name",
        "nombre_comercial": "trade_name",
        "identificacion": "identification",
        "ruc": "tax_id",
        "cedula": "id_card",
        "telefono": "phone",
        "correo": "email",
        "direccion": "address",
        
        # Transaction fields
        "transaccion": "transaction",
        "cobro": "collection",
        "pago": "payment",
        "retencion": "retention",
        "factura": "invoice",
        "nota_credito": "credit_note",
        "nota_debito": "debit_note",
        "guia_remision": "delivery_note",
        
        # Accounting fields
        "cuenta_contable": "accounting_account",
        "asiento": "journal_entry",
        "centro_costo": "cost_center",
        "debe": "debit",
        "haber": "credit",
        "saldo": "balance",
        
        # Bank fields
        "banco": "bank",
        "cuenta": "account",
        "movimiento": "movement",
        "deposito": "deposit",
        "retiro": "withdrawal",
        
        # Additional fields
        "activo": "active",
        "fecha_creacion": "created_at",
        "fecha_modificacion": "updated_at",
        "usuario": "user",
        "observaciones": "observations",
        "notas": "notes",
    }

    # Status translations
    STATUS_TRANSLATIONS = {
        "activo": "active",
        "inactivo": "inactive",
        "pendiente": "pending",
        "autorizado": "authorized",
        "anulado": "cancelled",
        "pagado": "paid",
        "por_cobrar": "receivable",
        "por_pagar": "payable",
    }

    # Document type translations
    DOCUMENT_TYPE_TRANSLATIONS = {
        "factura": "invoice",
        "nota_credito": "credit_note",
        "nota_debito": "debit_note",
        "guia_remision": "delivery_note",
        "retencion": "retention",
        "liquidacion_compra": "purchase_settlement",
    }

    @classmethod
    def translate_field_name(cls, spanish_name: str) -> str:
        """
        Translate a Spanish field name to English.
        
        Args:
            spanish_name: Spanish field name
            
        Returns:
            English field name (or original if no translation found)
        """
        return cls.FIELD_TRANSLATIONS.get(spanish_name.lower(), spanish_name)

    @classmethod
    def translate_dict(cls, data: Dict[str, Any], deep: bool = True) -> Dict[str, Any]:
        """
        Translate dictionary keys from Spanish to English.
        
        Args:
            data: Dictionary with Spanish keys
            deep: If True, recursively translate nested dictionaries
            
        Returns:
            Dictionary with English keys
        """
        if not isinstance(data, dict):
            return data
            
        translated = {}
        
        for key, value in data.items():
            # Translate the key
            english_key = cls.translate_field_name(key)
            
            # Recursively translate nested structures if deep=True
            if deep:
                if isinstance(value, dict):
                    value = cls.translate_dict(value, deep=True)
                elif isinstance(value, list):
                    value = [
                        cls.translate_dict(item, deep=True) if isinstance(item, dict) else item
                        for item in value
                    ]
            
            translated[english_key] = value
            
        return translated

    @classmethod
    def translate_list(cls, data: List[Dict[str, Any]], deep: bool = True) -> List[Dict[str, Any]]:
        """
        Translate a list of dictionaries from Spanish to English.
        
        Args:
            data: List of dictionaries with Spanish keys
            deep: If True, recursively translate nested dictionaries
            
        Returns:
            List of dictionaries with English keys
        """
        return [cls.translate_dict(item, deep=deep) for item in data]

    @classmethod
    def translate_status(cls, spanish_status: str) -> str:
        """
        Translate a status value from Spanish to English.
        
        Args:
            spanish_status: Spanish status value
            
        Returns:
            English status value (or original if no translation found)
        """
        return cls.STATUS_TRANSLATIONS.get(spanish_status.lower(), spanish_status)

    @classmethod
    def translate_document_type(cls, spanish_type: str) -> str:
        """
        Translate a document type from Spanish to English.
        
        Args:
            spanish_type: Spanish document type
            
        Returns:
            English document type (or original if no translation found)
        """
        return cls.DOCUMENT_TYPE_TRANSLATIONS.get(spanish_type.lower(), spanish_type)

    @classmethod
    def add_translation(cls, spanish: str, english: str, category: str = "field") -> None:
        """
        Add a new translation mapping.
        
        Args:
            spanish: Spanish term
            english: English translation
            category: Translation category ('field', 'status', or 'document_type')
        """
        if category == "field":
            cls.FIELD_TRANSLATIONS[spanish.lower()] = english
        elif category == "status":
            cls.STATUS_TRANSLATIONS[spanish.lower()] = english
        elif category == "document_type":
            cls.DOCUMENT_TYPE_TRANSLATIONS[spanish.lower()] = english
