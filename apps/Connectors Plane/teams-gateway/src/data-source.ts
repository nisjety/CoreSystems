import 'reflect-metadata';
import { DataSource } from 'typeorm';
import * as dotenv from 'dotenv';
import * as path from 'path';

// Load environment (prefer local for dev)
dotenv.config({ path: '.env.local' });
dotenv.config(); // fallback to .env if needed

const isAzurePostgres = (process.env.POSTGRES_HOST || '').includes('postgres.database.azure.com');

const AppDataSource = new DataSource({
  type: 'postgres',
  host: process.env.POSTGRES_HOST || 'localhost',
  port: Number(process.env.POSTGRES_PORT) || 5432,
  username: process.env.POSTGRES_USER || 'postgres',
  password: process.env.POSTGRES_PASSWORD || '',
  database: process.env.POSTGRES_DB || 'aquatiq_hub',
  entities: [path.join(__dirname, 'entities', '*.entity.{ts,js}')],
  migrations: [path.join(__dirname, 'migrations', '*.{ts,js}')],
  synchronize: false,
  logging: true,
  ssl: isAzurePostgres ? { rejectUnauthorized: false } : (process.env.DB_SSL === 'true'),
  extra: {
    options: '-c search_path=public',
  },
});

module.exports = AppDataSource;
module.exports.AppDataSource = AppDataSource;