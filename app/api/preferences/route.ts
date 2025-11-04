import { NextResponse } from "next/server";
import { getServerSession } from "next-auth";
import { authOptions } from "@/lib/auth";
import { prisma } from "@/lib/prisma";

// GET /api/preferences - Get user's saved preferences
export async function GET(req: Request) {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  try {
    const userId = parseInt(session.user.id);

    const user = await prisma.users.findUnique({
      where: { id: userId },
      select: { game_type_filters: true },
    });

    if (!user) {
      return NextResponse.json({ error: "User not found" }, { status: 404 });
    }

    // Parse the game_type_filters from JSON string to array
    let gameTypeFilters: string[] = [];
    if (user.game_type_filters) {
      try {
        gameTypeFilters = JSON.parse(user.game_type_filters);
      } catch (e) {
        console.error("Error parsing game_type_filters:", e);
        gameTypeFilters = [];
      }
    }

    return NextResponse.json({ gameTypeFilters });
  } catch (error) {
    console.error("Error fetching preferences:", error);
    return NextResponse.json(
      { error: "Failed to fetch preferences" },
      { status: 500 }
    );
  }
}

// POST /api/preferences - Save user's preferences
export async function POST(req: Request) {
  const session = await getServerSession(authOptions);

  if (!session?.user) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  try {
    const userId = parseInt(session.user.id);
    const { gameTypeFilters } = await req.json();

    // Validate that gameTypeFilters is an array
    if (!Array.isArray(gameTypeFilters)) {
      return NextResponse.json(
        { error: "gameTypeFilters must be an array" },
        { status: 400 }
      );
    }

    // Validate that each filter is one of the allowed values
    const allowedFilters = ["kids", "teen", "college"];
    const invalidFilters = gameTypeFilters.filter(
      (filter) => !allowedFilters.includes(filter)
    );

    if (invalidFilters.length > 0) {
      return NextResponse.json(
        { error: `Invalid filters: ${invalidFilters.join(", ")}` },
        { status: 400 }
      );
    }

    // Save to database as JSON string
    await prisma.users.update({
      where: { id: userId },
      data: {
        game_type_filters: JSON.stringify(gameTypeFilters),
      },
    });

    return NextResponse.json({ success: true, gameTypeFilters });
  } catch (error) {
    console.error("Error saving preferences:", error);
    return NextResponse.json(
      { error: "Failed to save preferences" },
      { status: 500 }
    );
  }
}
