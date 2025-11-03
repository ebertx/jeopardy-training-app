import { PrismaClient } from '@prisma/client';

const prisma = new PrismaClient();

async function main() {
  console.log('Running migration: add user roles and approval...');

  try {
    // Add role column
    await prisma.$executeRawUnsafe(`
      ALTER TABLE users
      ADD COLUMN IF NOT EXISTS role VARCHAR(20) DEFAULT 'user'
    `);
    console.log('✓ Added role column');

    // Add approved column
    await prisma.$executeRawUnsafe(`
      ALTER TABLE users
      ADD COLUMN IF NOT EXISTS approved BOOLEAN DEFAULT FALSE
    `);
    console.log('✓ Added approved column');

    // Add approved_at column
    await prisma.$executeRawUnsafe(`
      ALTER TABLE users
      ADD COLUMN IF NOT EXISTS approved_at TIMESTAMP
    `);
    console.log('✓ Added approved_at column');

    // Add indexes
    await prisma.$executeRawUnsafe(`
      CREATE INDEX IF NOT EXISTS users_approved_idx ON users(approved)
    `);
    console.log('✓ Added approved index');

    await prisma.$executeRawUnsafe(`
      CREATE INDEX IF NOT EXISTS users_role_idx ON users(role)
    `);
    console.log('✓ Added role index');

    // Make first user an approved admin
    await prisma.$executeRawUnsafe(`
      UPDATE users
      SET approved = TRUE, role = 'admin', approved_at = NOW()
      WHERE id = 1
    `);
    console.log('✓ Set user ID 1 as approved admin');

    console.log('\n✅ Migration completed successfully!');
  } catch (error) {
    console.error('❌ Migration failed:', error);
    throw error;
  } finally {
    await prisma.$disconnect();
  }
}

main();
