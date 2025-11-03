import { Resend } from 'resend';

let resend: Resend | null = null;

function getResend(): Resend {
  if (!resend) {
    resend = new Resend(process.env.RESEND_API_KEY);
  }
  return resend;
}

export async function sendNewUserNotificationToAdmin(
  username: string,
  email: string,
  userId: number
) {
  if (!process.env.RESEND_API_KEY || !process.env.ADMIN_EMAIL) {
    console.log('Email not configured - skipping admin notification');
    return;
  }

  try {
    await getResend().emails.send({
      from: process.env.EMAIL_FROM || 'Jeopardy Training <noreply@jeopardy.ebertx.com>',
      to: process.env.ADMIN_EMAIL,
      subject: 'New User Registration Pending Approval',
      html: `
        <div style="font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto;">
          <h2 style="color: #060CE9;">New User Registration</h2>
          <p>A new user has registered and is awaiting approval:</p>

          <div style="background-color: #f5f5f5; padding: 15px; border-radius: 5px; margin: 20px 0;">
            <p><strong>Username:</strong> ${username}</p>
            <p><strong>Email:</strong> ${email}</p>
            <p><strong>User ID:</strong> ${userId}</p>
          </div>

          <p>To approve this user, visit:</p>
          <a href="https://jeopardy.ebertx.com/admin"
             style="display: inline-block; background-color: #060CE9; color: white; padding: 12px 24px; text-decoration: none; border-radius: 5px; margin: 10px 0;">
            Go to Admin Dashboard
          </a>

          <p style="color: #666; font-size: 12px; margin-top: 30px;">
            This is an automated notification from your Jeopardy Training application.
          </p>
        </div>
      `,
    });
    console.log(`Admin notification sent for new user: ${username}`);
  } catch (error) {
    console.error('Failed to send admin notification:', error);
  }
}

export async function sendApprovalNotificationToUser(
  username: string,
  email: string
) {
  if (!process.env.RESEND_API_KEY) {
    console.log('Email not configured - skipping user notification');
    return;
  }

  try {
    await getResend().emails.send({
      from: process.env.EMAIL_FROM || 'Jeopardy Training <noreply@jeopardy.ebertx.com>',
      to: email,
      subject: 'Your Account Has Been Approved!',
      html: `
        <div style="font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto;">
          <h2 style="color: #060CE9;">Account Approved!</h2>
          <p>Hi ${username},</p>

          <p>Great news! Your Jeopardy Training account has been approved. You can now log in and start practicing.</p>

          <a href="https://jeopardy.ebertx.com/login"
             style="display: inline-block; background-color: #060CE9; color: white; padding: 12px 24px; text-decoration: none; border-radius: 5px; margin: 20px 0;">
            Log In Now
          </a>

          <div style="background-color: #f5f5f5; padding: 15px; border-radius: 5px; margin: 20px 0;">
            <h3 style="margin-top: 0;">What's Available:</h3>
            <ul>
              <li>500,000+ Jeopardy questions</li>
              <li>Mastery-based learning system</li>
              <li>Performance analytics by category</li>
              <li>AI-powered study recommendations</li>
            </ul>
          </div>

          <p>Happy studying!</p>

          <p style="color: #666; font-size: 12px; margin-top: 30px;">
            If you didn't request this account, please ignore this email.
          </p>
        </div>
      `,
    });
    console.log(`Approval notification sent to: ${email}`);
  } catch (error) {
    console.error('Failed to send approval notification:', error);
  }
}
