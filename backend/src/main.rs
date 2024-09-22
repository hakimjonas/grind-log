use actix_web::{web, App, HttpResponse, HttpServer, Responder, Result};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use dotenv::dotenv;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};

// Data models
#[derive(Serialize, Deserialize, Clone, FromRow)]
struct Session {
    id: i64,
    date: String,         // Format: "YYYY-MM-DD"
    session_type: String, // "1-hour", "2-hours", "3-hours"
}

#[derive(Serialize, Deserialize)]
struct TimeResponse {
    current_time: String,
    streak: usize,
    total_points: usize,
    date: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Deserialize)]
struct SessionLog {
    date: String,         // Format: "YYYY-MM-DD"
    session_type: String, // "1-hour", "2-hours", "3-hours"
}

#[derive(Serialize)]
struct WeeklyActivity {
    week_start: String, // Start date of the week
    points: usize,      // Total points for the week
}

// New structs for achievements and streaks
#[derive(Serialize)]
struct StreaksResponse {
    overall_streak: usize,
    yearly_streak: usize,
    monthly_streak: usize,
}

#[derive(Serialize, Deserialize)]
struct StreakBonusResponse {
    streak_length: usize,
    week_start: String, // Use a string representation of the week start date
}

#[derive(Serialize)]
struct AchievementsResponse {
    achievements: Vec<String>,
}

#[derive(Serialize)]
struct StatisticsResponse {
    current_date: String,
    streak: usize,
    total_points: usize,
    weekly_trend: Vec<WeeklyActivity>,
    achievements: Vec<String>,
    yearly_streak: usize,
    monthly_streak: usize,
}

// Custom error type
#[derive(Debug)]
pub enum ApiError {
    InvalidInput(String),
    DatabaseError(String),
    SerializationError(String),
}

impl Display for ApiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::InvalidInput(message) => write!(f, "Invalid input: {}", message),
            ApiError::DatabaseError(message) => write!(f, "Database error: {}", message),
            ApiError::SerializationError(message) => write!(f, "Serialization error: {}", message),
        }
    }
}

// Implement ResponseError for ApiError
impl actix_web::error::ResponseError for ApiError {
    fn error_response(&self) -> HttpResponse {
        match self {
            ApiError::InvalidInput(message) => HttpResponse::BadRequest().json(ErrorResponse {
                error: message.clone(),
            }),
            ApiError::DatabaseError(message) => {
                HttpResponse::InternalServerError().json(ErrorResponse {
                    error: message.clone(),
                })
            }
            ApiError::SerializationError(message) => {
                HttpResponse::InternalServerError().json(ErrorResponse {
                    error: message.clone(),
                })
            }
        }
    }
}

// Fetch the current time and calculate streaks
pub async fn get_time(
    pool: web::Data<sqlx::SqlitePool>,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse, actix_web::Error> {
    let current_time = if let Some(date_str) = query.get("date") {
        NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .map_err(|_| actix_web::error::ErrorBadRequest("Invalid date format"))?
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| actix_web::error::ErrorBadRequest("Invalid time components"))?
    } else {
        Utc::now().naive_utc()
    };

    // Fetch sessions to calculate streak and total points
    let (sessions, _, total_points) = fetch_sessions(&pool)
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;
    let (streak, _) = calculate_streak_and_points(&sessions)
        .map_err(|e| actix_web::error::ErrorInternalServerError(e.to_string()))?;

    Ok(HttpResponse::Ok().json(TimeResponse {
        current_time: current_time.to_string(),
        streak,
        total_points,
        date: current_time.date().to_string(),
    }))
}

async fn create_session(
    pool: web::Data<sqlx::SqlitePool>,
    session_log: web::Json<SessionLog>,
) -> Result<impl Responder, ApiError> {
    let date = parse_date(&session_log.date)?;
    let valid_session_types = ["1-hour", "2-hours", "3-hours"];
    if !valid_session_types.contains(&session_log.session_type.as_str()) {
        return Err(ApiError::InvalidInput("Invalid session type".into()));
    }

    // Insert session into the database
    sqlx::query("INSERT INTO session (date, session_type) VALUES (?, ?)")
        .bind(&session_log.date)
        .bind(&session_log.session_type)
        .execute(pool.get_ref())
        .await
        .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

    let (sessions, _, total_points) = fetch_sessions(pool.get_ref()).await?;
    let (streak, _) = calculate_streak_and_points(&sessions)?;

    Ok(HttpResponse::Created().json(TimeResponse {
        current_time: session_log.date.clone(),
        streak,
        total_points,
        date: date.to_string(), // Use the parsed date
    }))
}

// Fetch sessions stored in the database
async fn fetch_sessions(
    pool: &sqlx::SqlitePool,
) -> Result<(Vec<Session>, String, usize), ApiError> {
    let sessions: Vec<Session> =
        sqlx::query_as::<_, Session>("SELECT * FROM session ORDER BY date")
            .fetch_all(pool)
            .await
            .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

    let current_date = if let Some(last_session) = sessions.last() {
        last_session.date.clone()
    } else {
        Utc::now().date_naive().to_string()
    };

    let total_points: usize = sessions
        .iter()
        .map(|s| calculate_session_points(&s.session_type))
        .sum();

    Ok((sessions, current_date, total_points))
}

fn parse_date(date_str: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .map_err(|_| ApiError::InvalidInput("Invalid date format".into()))
}

fn calculate_session_points(session_type: &str) -> usize {
    match session_type {
        "1-hour" => 10,
        "2-hours" => 12,
        "3-hours" => 14,
        _ => 0,
    }
}

fn update_streak(last_date: Option<NaiveDate>, current_date: NaiveDate, streak: usize) -> usize {
    match last_date {
        Some(last) if (current_date - last).num_days() == 1 => streak + 1,
        _ => 1, // Reset the streak if it's not consecutive
    }
}

fn calculate_streak_and_points(sessions: &[Session]) -> Result<(usize, usize), ApiError> {
    // Fold function to calculate streak and points
    let fold_fn = |(streak, total_points, last_date): (usize, usize, Option<NaiveDate>),
                   session: &Session|
                   -> Result<(usize, usize, Option<NaiveDate>), ApiError> {
        let current_date = parse_date(&session.date)?;
        let session_points = calculate_session_points(&session.session_type);
        let new_streak = update_streak(last_date, current_date, streak);

        Ok((
            new_streak,
            total_points + session_points,
            Some(current_date),
        ))
    };

    sessions
        .iter()
        .sorted_by(|a, b| a.date.cmp(&b.date)) // Sort sessions by date
        .try_fold((0, 0, None), fold_fn) // Fold with error handling
        .map(|(streak, total_points, _)| (streak, total_points)) // Ignore last_date in the result
}

fn get_week_start(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

fn calculate_trend(sessions: &[Session], period: &str) -> Vec<WeeklyActivity> {
    sessions
        .iter()
        .map(|session| {
            let date = parse_date(&session.date).unwrap();
            let period_start = match period {
                "week" => get_week_start(date).to_string(),
                "month" => format!("{}-{:02}", date.year(), date.month()),
                "year" => date.year().to_string(),
                _ => panic!("Invalid period"),
            };
            let points = calculate_session_points(&session.session_type);
            (period_start, points)
        })
        .sorted_by(|(a_period_start, _), (b_period_start, _)| a_period_start.cmp(b_period_start))
        .chunk_by(|(period_start, _)| period_start.clone())
        .into_iter()
        .map(|(period_start, group)| {
            let total_points = group.map(|(_, points)| points).sum();
            WeeklyActivity {
                week_start: period_start,
                points: total_points,
            }
        })
        .collect()
}

fn calculate_period_streak<F>(sessions: &[Session], filter_fn: F) -> usize
where
    F: Fn(NaiveDate) -> bool,
{
    sessions
        .iter()
        .filter(|session| {
            if let Ok(date) = parse_date(&session.date) {
                filter_fn(date)
            } else {
                false
            }
        })
        .count()
}

type StatisticsResult = (Vec<WeeklyActivity>, Vec<String>, usize, usize, usize);

fn calculate_statistics(
    sessions: &[Session],
    current_date: NaiveDate,
) -> std::result::Result<StatisticsResult, ApiError> {
    let weekly_trend: Vec<WeeklyActivity> = calculate_trend(sessions, "week");

    // Calculate overall streak directly
    let (overall_streak, _total_points) = calculate_streak_and_points(sessions)?;

    // Determine achievements based on the overall streak
    let achievements = if overall_streak >= 7 {
        vec!["7-day streak".to_string()]
    } else {
        Vec::new()
    };

    // Calculate yearly streak
    let current_year = current_date.year();
    let yearly_streak = calculate_period_streak(sessions, |date| date.year() == current_year);

    // Calculate monthly streak using the utility function
    let current_month = current_date.month();
    let monthly_streak = calculate_period_streak(sessions, |date| date.month() == current_month);

    Ok((
        weekly_trend,
        achievements,
        overall_streak,
        yearly_streak,
        monthly_streak,
    ))
}

// Additional endpoint implementations
async fn get_weekly_trend(pool: web::Data<sqlx::SqlitePool>) -> Result<impl Responder, ApiError> {
    let (sessions, _current_date, _total_points) = fetch_sessions(&pool).await?;
    let (weekly_trend, _, _, _, _) = calculate_statistics(&sessions, Default::default())?;

    Ok(HttpResponse::Ok().json(weekly_trend))
}

async fn get_achievements(pool: web::Data<sqlx::SqlitePool>) -> Result<impl Responder, ApiError> {
    let (sessions, _current_date, _total_points) = fetch_sessions(&pool).await?;
    let (_, achievements, _, _, _) = calculate_statistics(&sessions, Default::default())?;

    Ok(HttpResponse::Ok().json(AchievementsResponse { achievements }))
}

async fn get_streaks(pool: web::Data<sqlx::SqlitePool>) -> Result<impl Responder, ApiError> {
    let (sessions, _current_date, _total_points) = fetch_sessions(&pool).await?;
    let (_, _, overall_streak, yearly_streak, monthly_streak) =
        calculate_statistics(&sessions, Default::default())?;

    Ok(HttpResponse::Ok().json(StreaksResponse {
        overall_streak,
        yearly_streak,
        monthly_streak,
    }))
}

async fn get_streak_bonuses(pool: web::Data<sqlx::SqlitePool>) -> Result<impl Responder, ApiError> {
    let (sessions, _current_date, _total_points) = fetch_sessions(&pool).await?;
    let bonuses = calculate_weekly_streak_bonus(&sessions);

    Ok(HttpResponse::Ok().json(bonuses))
}

fn calculate_weekly_streak_bonus(sessions: &[Session]) -> Vec<StreakBonusResponse> {
    let (finalized_streaks, last_date, last_streak) = sessions
        .iter()
        .map(|session| parse_date(&session.date).unwrap()) // Parse dates
        .fold(
            (Vec::new(), None, 0), // (weekly_streaks, last_date, current_streak)
            |(weekly_streaks, last_date, streak), session_date| {
                let week_start = get_week_start(session_date);

                // Check if we're in a new week
                let is_new_week =
                    last_date.map_or(false, |last| week_start != get_week_start(last));

                let updated_weekly_streaks = if is_new_week && streak >= 3 {
                    weekly_streaks
                        .into_iter()
                        .chain(Some((streak, get_week_start(last_date.unwrap())))) // Add streak for the previous week
                        .collect()
                } else {
                    weekly_streaks
                };

                // Calculate the new streak
                let new_streak = match last_date {
                    Some(last) if (session_date - last).num_days() == 1 => streak + 1,
                    _ => 1,
                };

                (updated_weekly_streaks, Some(session_date), new_streak)
            },
        );

    // Include the final streak if it qualifies (length >= 3)
    if last_streak >= 3 {
        finalized_streaks
            .into_iter()
            .chain(Some((last_streak, get_week_start(last_date.unwrap()))))
            .map(|(length, week_start)| StreakBonusResponse {
                streak_length: length,
                week_start: week_start.to_string(),
            })
            .collect()
    } else {
        finalized_streaks
            .into_iter()
            .map(|(length, week_start)| StreakBonusResponse {
                streak_length: length,
                week_start: week_start.to_string(),
            })
            .collect()
    }
}

async fn get_overall_statistics(
    pool: web::Data<sqlx::SqlitePool>,
) -> Result<impl Responder, ApiError> {
    let (sessions, current_date, total_points) = fetch_sessions(&pool).await?;
    let (weekly_trend, achievements, overall_streak, yearly_streak, monthly_streak) =
        calculate_statistics(&sessions, Default::default())?;

    Ok(HttpResponse::Ok().json(StatisticsResponse {
        current_date,
        streak: overall_streak,
        total_points,
        weekly_trend,
        achievements,
        yearly_streak,
        monthly_streak,
    }))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok(); // Ensure this line is present

    let database_url = "sqlite::memory:";

    // Print the database URL for debugging purposes
    println!("Using database URL: {}", database_url);

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to create pool.");

    // Initialize the database
    init_db(&pool)
        .await
        .expect("Failed to initialize database.");

    // Share the pool across routes
    let pool = web::Data::new(pool);

    HttpServer::new(move || {
        App::new()
            .app_data(pool.clone())
            .route("/", web::get().to(api_docs)) // Add the documentation endpoint
            .route("/api/time", web::get().to(get_time))
            .route("/api/log_session", web::post().to(create_session))
            .route(
                "/api/statistics/weekly_trend",
                web::get().to(get_weekly_trend),
            )
            .route(
                "/api/statistics/achievements",
                web::get().to(get_achievements),
            )
            .route("/api/statistics/streaks", web::get().to(get_streaks))
            .route(
                "/api/statistics/overall",
                web::get().to(get_overall_statistics),
            )
            .route("/api/bonuses/streaks", web::get().to(get_streak_bonuses))
    })
        .bind("127.0.0.1:8080")?
        .run()
        .await
}

async fn init_db(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS session (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL,
            session_type TEXT NOT NULL
        );
        "#,
    )
        .execute(pool)
        .await?;

    Ok(())
}

/// Generate an API documentation page
async fn api_docs() -> impl Responder {
    let doc_content = r#"
        <html>
        <head>
            <title>API Documentation</title>
            <style>
                body { font-family: Arial, sans-serif; margin: 20px; }
                h1 { color: #333; }
                table { width: 100%; border-collapse: collapse; margin-top: 20px; }
                th, td { border: 1px solid #ddd; padding: 8px; text-align: left; }
                th { background-color: #f2f2f2; }
                pre { background-color: #f8f8f8; padding: 10px; }
            </style>
        </head>
        <body>
            <h1>API Documentation</h1>
            <table>
                <thead>
                    <tr>
                        <th>Endpoint</th>
                        <th>Method</th>
                        <th>Description</th>
                        <th>Request Example</th>
                        <th>Response Example</th>
                    </tr>
                </thead>
                <tbody>
                    <tr>
                        <td>/api/time</td>
                        <td>GET</td>
                        <td>Fetch the current time and calculate streaks.</td>
                        <td>N/A</td>
                        <td><pre>{ "current_time": "2023-10-01T00:00:00Z", "streak": 3, "total_points": 36, "date": "2023-10-01" }</pre></td>
                    </tr>
                    <tr>
                        <td>/api/log_session</td>
                        <td>POST</td>
                        <td>Create a new session log entry.</td>
                        <td><pre>{ "date": "2023-10-01", "session_type": "1-hour" }</pre></td>
                        <td><pre>{ "current_time": "2023-10-01T00:00:00Z", "streak": 3, "total_points": 36, "date": "2023-10-01" }</pre></td>
                    </tr>
                    <tr>
                        <td>/api/statistics/weekly_trend</td>
                        <td>GET</td>
                        <td>Retrieve weekly trends for sessions.</td>
                        <td>N/A</td>
                        <td><pre>[{ "week_start": "2023-09-25", "points": 36 }]</pre></td>
                    </tr>
                    <tr>
                        <td>/api/statistics/achievements</td>
                        <td>GET</td>
                        <td>Fetch user achievements.</td>
                        <td>N/A</td>
                        <td><pre>{ "achievements": [] }</pre></td>
                    </tr>
                    <tr>
                        <td>/api/statistics/streaks</td>
                        <td>GET</td>
                        <td>Fetch overall, yearly, and monthly streaks.</td>
                        <td>N/A</td>
                        <td><pre>{ "overall_streak": 3, "yearly_streak": 3, "monthly_streak": 3 }</pre></td>
                    </tr>
                    <tr>
                        <td>/api/statistics/overall</td>
                        <td>GET</td>
                        <td>Retrieve overall statistics.</td>
                        <td>N/A</td>
                        <td><pre>{ "current_date": "2023-10-01", "streak": 3, "total_points": 36, "weekly_trend": [], "achievements": [], "yearly_streak": 3, "monthly_streak": 3 }</pre></td>
                    </tr>
                    <tr>
                        <td>/api/bonuses/streaks</td>
                        <td>GET</td>
                        <td>Get streak bonuses for the current week.</td>
                        <td>N/A</td>
                        <td><pre>[{ "streak_length": 3, "week_start": "2023-09-25" }]</pre></td>
                    </tr>
                </tbody>
            </table>
        </body>
        </html>
    "#;

    HttpResponse::Ok()
        .content_type("text/html")
        .body(doc_content)
}
// Tests
#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test;
    use chrono::NaiveDate;

    // Sample sessions for testing
    fn sample_sessions() -> Vec<Session> {
        vec![
            Session {
                id: 1,
                date: "2023-10-01".to_string(),
                session_type: "1-hour".to_string(),
            },
            Session {
                id: 2,
                date: "2023-10-02".to_string(),
                session_type: "2-hours".to_string(),
            },
            Session {
                id: 3,
                date: "2023-10-03".to_string(),
                session_type: "3-hours".to_string(),
            },
        ]
    }

    #[test]
    async fn test_parse_date_valid() {
        let date = parse_date("2023-10-01").unwrap();
        assert_eq!(date.to_string(), "2023-10-01");
    }

    #[test]
    async fn test_parse_date_invalid() {
        let result = parse_date("invalid-date");
        assert!(result.is_err());
    }

    #[test]
    async fn test_calculate_session_points() {
        assert_eq!(calculate_session_points("1-hour"), 10);
        assert_eq!(calculate_session_points("2-hours"), 12);
        assert_eq!(calculate_session_points("3-hours"), 14);
        assert_eq!(calculate_session_points("invalid"), 0);
    }

    #[test]
    async fn test_calculate_streak_and_points() {
        let sessions = sample_sessions();
        let (streak, total_points) = calculate_streak_and_points(&sessions).unwrap();
        assert_eq!(streak, 3);
        assert_eq!(total_points, 36); // 10 + 12 + 14
    }

    #[test]
    async fn test_calculate_weekly_streak_bonus() {
        let sessions = sample_sessions();

        let weekly_streak_bonuses = calculate_weekly_streak_bonus(&sessions);

        // Assert that we have one streak bonus
        assert_eq!(weekly_streak_bonuses.len(), 1);

        // Check the streak length
        assert_eq!(weekly_streak_bonuses[0].streak_length, 3);

        // Check the week start date
        assert_eq!(weekly_streak_bonuses[0].week_start, "2023-10-02");
    }

    #[test]
    async fn test_calculate_statistics() {
        // Set the mock date to October 15, 2023
        let current_date = NaiveDate::from_ymd_opt(2023, 10, 15).unwrap();

        let sessions = sample_sessions();

        let (weekly_trend, achievements, overall_streak, yearly_streak, monthly_streak) =
            calculate_statistics(&sessions, current_date).unwrap();

        // Ensure the weekly trends are correctly calculated
        assert_eq!(weekly_trend.len(), 2);
        assert_eq!(weekly_trend[0].points, 10);
        assert_eq!(weekly_trend[1].points, 26);

        // Check achievements (since there's no 7-day streak in this example)
        assert!(achievements.is_empty());

        // Check the overall streak
        assert_eq!(overall_streak, 3); // Expect a streak of 3

        // Check yearly and monthly streaks
        assert_eq!(yearly_streak, 3); // Expect a yearly streak of 3
        assert_eq!(monthly_streak, 3); // Expect a monthly streak of 3
    }

    // Integration tests
    mod integration_tests {
        use super::*;
        use actix_web::{test, App};

        #[actix_rt::test]
        async fn test_get_time_endpoint() {
            // Set up an in-memory SQLite database for testing
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .connect(":memory:")
                .await
                .unwrap();

            init_db(&pool).await.unwrap();

            // Insert sample sessions into the test database
            let sample_sessions = vec![
                SessionLog {
                    date: "2023-10-01".to_string(),
                    session_type: "1-hour".to_string(),
                },
                SessionLog {
                    date: "2023-10-02".to_string(),
                    session_type: "2-hours".to_string(),
                },
            ];

            for session in sample_sessions {
                sqlx::query("INSERT INTO session (date, session_type) VALUES (?, ?)")
                    .bind(&session.date)
                    .bind(&session.session_type)
                    .execute(&pool)
                    .await
                    .unwrap();
            }

            let app = test::init_service(
                App::new()
                    .app_data(web::Data::new(pool.clone()))
                    .route("/api/time", web::get().to(get_time)),
            )
                .await;

            let req = test::TestRequest::get().uri("/api/time").to_request();
            let resp: TimeResponse = test::call_and_read_body_json(&app, req).await;

            // Assert to confirm streak calculation works correctly
            assert_eq!(resp.streak, 2); // Expecting a streak of 2 from the inserted sessions
            assert_eq!(resp.total_points, 22); // 10 + 12 points
        }

        #[actix_rt::test]
        async fn test_get_streak_bonuses_endpoint() {
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .connect(":memory:")
                .await
                .unwrap();

            init_db(&pool).await.unwrap();

            // Add sample data to the database
            let sample_data = sample_sessions();
            for session in sample_data {
                sqlx::query("INSERT INTO session (date, session_type) VALUES (?, ?)")
                    .bind(&session.date)
                    .bind(&session.session_type)
                    .execute(&pool)
                    .await
                    .unwrap();
            }

            let app = test::init_service(
                App::new()
                    .app_data(web::Data::new(pool.clone()))
                    .route("/api/bonuses/streaks", web::get().to(get_streak_bonuses)),
            )
                .await;

            let req = test::TestRequest::get()
                .uri("/api/bonuses/streaks")
                .to_request();
            let resp: Vec<StreakBonusResponse> = test::call_and_read_body_json(&app, req).await;

            assert_eq!(resp.len(), 1);
            assert_eq!(resp[0].streak_length, 3);
            assert_eq!(resp[0].week_start, "2023-10-02");
        }
    }
}
