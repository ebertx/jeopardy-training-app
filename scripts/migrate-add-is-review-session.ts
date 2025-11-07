import { prisma } from '../lib/prisma';

async function main() {
  console.log('Adding is_review_session column to quiz_sessions...');

  try {
    // Add the column
    await prisma.$executeRawUnsafe(`
      ALTER TABLE quiz_sessions
      ADD COLUMN IF NOT EXISTS is_review_session BOOLEAN NOT NULL DEFAULT false;
    `);
    console.log('✓ Column added successfully');

    // Add the index
    await prisma.$executeRawUnsafe(`
      CREATE INDEX IF NOT EXISTS quiz_sessions_user_id_is_review_session_idx
      ON quiz_sessions(user_id, is_review_session);
    `);
    console.log('✓ Index created successfully');

    console.log('\nMigration completed successfully!');
  } catch (error) {
    console.error('Error running migration:', error);
    process.exit(1);
  } finally {
    await prisma.$disconnect();
  }
}

main();
