Her er en mer hierarkisk og smartere oversikt over tjenestene, gruppert etter deres funksjoner:
Frontend
Service	Local Port	Environment Variable	Default	Status	Description
Frontend	3001	-	3001	🟡 Standby	Next.js React application

AI Tjenester
Service	Local Port	Environment Variable	Default	Status	Description
AI Orchestrator	3021	PORT	3021	✅ Active	Core AI processing service
Teams Gateway	3031	PORT	3031	✅ Active	Microsoft Teams bot service


Forretningsprosesser
Service	Local Port	Environment Variable	Default	Status	Description
Chat Server	3017	PORT	3017	✅ Active	Gateway service for frontend
Approval Service	3051	-	3051	✅ Active	Approval workflow service
Assets Service	3041	-	3041	✅ Active	Asset management service
Auth Service	3011	-	3011	✅ Active	Authentication service
HR Service	3022	-	3022	✅ Active	Human resources service
Procurement Service	3042	-	3042	✅ Active	Procurement management service
Time Bank Service	3023	-	3023	✅ Active	Time banking service
Corebare Service	3013	-	3013	✅ Active	Core backend service
Connectors	3016	-	3016	✅ Active	Data connectors from external sources (f.eks. Yr, Statens vegvesen, VG)


Eksterne Tjenester (I bruk)
Service	Local Port	Environment Variable	Default	Status	Description
PostgreSQL	5432	-	5432	✅ Active	Relational database service
Redis	6379	-	6379	✅ Active	In-memory data structure store


Eksterne Tjenester (Ikke i bruk)
Service	Local Port	Environment Variable	Default	Status	Description
NATS	4222	-	4222	🟡 Standby	Messaging system
Kafka	9092	-	9092	🟡 Standbye	Distributed event streaming platform
RabbitMQ	5672	-	5672	🟡 Standby	Message broker service
Ngrok	4040	-	4040	🟡 Standby	Secure tunnel to localhost
