#!/usr/bin/env python3
"""
Convert SuperOffice XML RSA keys to PEM format.

SuperOffice provides keys in XML format (for .NET), but Python's cryptography
library requires PEM format. This script converts them.
"""

import base64
import sys
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.backends import default_backend
import xml.etree.ElementTree as ET


def xml_to_private_key_pem(xml_file: str, output_file: str, password: str = None):
    """Convert XML private key to PEM format."""
    
    # Read XML file
    tree = ET.parse(xml_file)
    root = tree.getroot()
    
    # Extract components (all are base64 encoded)
    modulus = int.from_bytes(base64.b64decode(root.find('Modulus').text), 'big')
    exponent = int.from_bytes(base64.b64decode(root.find('Exponent').text), 'big')
    d = int.from_bytes(base64.b64decode(root.find('D').text), 'big')
    p = int.from_bytes(base64.b64decode(root.find('P').text), 'big')
    q = int.from_bytes(base64.b64decode(root.find('Q').text), 'big')
    
    # Calculate additional required values
    dmp1 = int.from_bytes(base64.b64decode(root.find('DP').text), 'big')
    dmq1 = int.from_bytes(base64.b64decode(root.find('DQ').text), 'big')
    iqmp = int.from_bytes(base64.b64decode(root.find('InverseQ').text), 'big')
    
    # Create RSA private key
    public_numbers = rsa.RSAPublicNumbers(exponent, modulus)
    private_numbers = rsa.RSAPrivateNumbers(
        p=p, q=q, d=d,
        dmp1=dmp1, dmq1=dmq1, iqmp=iqmp,
        public_numbers=public_numbers
    )
    private_key = private_numbers.private_key(default_backend())
    
    # Determine encryption
    encryption = serialization.NoEncryption()
    if password:
        encryption = serialization.BestAvailableEncryption(password.encode())
    
    # Write PEM file
    pem = private_key.private_bytes(
        encoding=serialization.Encoding.PEM,
        format=serialization.PrivateFormat.PKCS8,
        encryption_algorithm=encryption
    )
    
    with open(output_file, 'wb') as f:
        f.write(pem)
    
    print(f"✓ Converted private key to PEM: {output_file}")
    if password:
        print(f"  (Encrypted with password)")


def main():
    secrets_dir = Path(__file__).parent / "app" / "secrets"
    xml_private = secrets_dir / "privatekey.pem"  # Actually XML
    pem_private = secrets_dir / "privatekey_converted.pem"
    
    if not xml_private.exists():
        print(f"❌ Error: {xml_private} not found")
        sys.exit(1)
    
    print("\n🔄 Converting SuperOffice XML keys to PEM format...\n")
    
    # Ask for password
    print("Do you want to encrypt the PEM key with a password?")
    print("(Recommended for production, but you'll need to set SOCRM_PRIVATE_KEY_PASSWORD)")
    encrypt = input("Encrypt? (y/N): ").strip().lower()
    
    password = None
    if encrypt == 'y':
        password = input("Enter password: ").strip()
    
    # Convert private key
    try:
        xml_to_private_key_pem(str(xml_private), str(pem_private), password)
        
        # Set permissions
        pem_private.chmod(0o600)
        
        print("\n✅ Conversion complete!")
        print(f"\nNext steps:")
        print(f"1. Backup original: mv {xml_private} {xml_private}.xml.backup")
        print(f"2. Rename converted: mv {pem_private} {xml_private}")
        print(f"3. Delete publickey.pem (not needed)")
        print(f"4. Download SuperOffice certificate:")
        print(f"   curl -o cert.crt https://sod.superoffice.com/login/common/sod2stage-superoffice-com.crt")
        print(f"   openssl x509 -in cert.crt -out {secrets_dir}/SuperOfficeFederatedLogin.pem -outform PEM")
        
    except Exception as e:
        print(f"❌ Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
