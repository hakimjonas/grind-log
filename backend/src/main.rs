use actix_web::{web, App, HttpResponse, HttpServer, Responder, Result};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use dotenv::dotenv;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json::from_str;
use sled::Db;
use std::fmt::{Display, Formatter};
use std::str::from_utf8;
use std::sync::Arc;

// Data models
#[derive(Serialize, Deserialize, Clone)]
struct Session {
    date: String,         // Format: "YYYY-MM-DD"
    session_type: String, // "1-hour", "2-hours", "3-hours"
}

#[derive(Serialize, Deserialize)]
struct TimeResponse {
    current_time: String,
    streak: usize,
    total_points: usize,
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
}

impl Display for ApiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::InvalidInput(message) => write!(f, "Invalid input: {}", message),
            ApiError::DatabaseError(message) => write!(f, "Database error: {}", message),
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
        }
    }
}

// Fetch the current time and calculate streaks
pub async fn get_time(db: web::Data<Arc<Db>>) -> Result<impl Responder, ApiError> {
    let time = Utc::now();
    let time_str = time.to_rfc3339(); // ISO 8601 format

    db.insert(b"last_time", time_str.as_bytes())
        .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

    let (sessions, _, total_points) = fetch_sessions(&db)?;
    let (streak, _) = calculate_streak_and_points(&sessions)?;

    Ok(HttpResponse::Ok().json(TimeResponse {
        current_time: time_str,
        streak,
        total_points,
    }))
}

async fn create_session(
    db: web::Data<Arc<Db>>,
    session_log: web::Json<SessionLog>,
) -> Result<impl Responder, ApiError> {
    let date = parse_date(&session_log.date)?;
    let valid_session_types = ["1-hour", "2-hours", "3-hours"];
    if !valid_session_types.contains(&session_log.session_type.as_str()) {
        return Err(ApiError::InvalidInput("Invalid session type".into()));
    }

    let session = Session {
        date: date.to_string(),
        session_type: session_log.session_type.clone(),
    };

    let session_str =
        serde_json::to_string(&session).map_err(|e| ApiError::DatabaseError(e.to_string()))?;
    db.insert(session.date.as_bytes(), session_str.as_bytes())
        .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

    let (sessions, _, total_points) = fetch_sessions(&db)?;
    let (streak, _) = calculate_streak_and_points(&sessions)?;

    Ok(HttpResponse::Created().json(TimeResponse {
        current_time: session.date,
        streak,
        total_points,
    }))
}

// Fetch sessions stored in the database
fn fetch_sessions(
    db: &web::Data<Arc<Db>>,
) -> std::result::Result<(Vec<Session>, String, usize), ApiError> {
    let sessions: Vec<Session> = db
        .iter()
        .map(|result| {
            result
                .map_err(|e| ApiError::DatabaseError(e.to_string()))
                .and_then(|(_key, value)| {
                    from_utf8(&value)
                        .map_err(|e| ApiError::DatabaseError(e.to_string()))
                        .and_then(|session_str| {
                            from_str(session_str)
                                .map_err(|e| ApiError::DatabaseError(e.to_string()))
                        })
                })
        })
        .collect::<Result<_, _>>()?;

    // Calculate current date from the last session
    let current_date = if let Some(last_session) = sessions.last() {
        last_session.date.clone()
    } else {
        Utc::now().date_naive().to_string() // Using Utc::now().date_naive() instead of Utc::today()
    };

    // Calculate total points
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
    use itertools::Itertools;

    // Fold function to calculate streak and points
    let fold_fn = |(streak, total_points, last_date): (usize, usize, Option<NaiveDate>),
                   session: Session|
                   -> Result<(usize, usize, Option<NaiveDate>), ApiError> {
        let current_date = parse_date(&session.date)?;
        let session_points = calculate_session_points(&session.session_type);
        let new_streak = update_streak(last_date, current_date, streak); // Pass current streak to function

        Ok((
            new_streak,
            total_points + session_points,
            Some(current_date),
        ))
    };

    sessions
        .iter()
        .cloned()
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

fn calculate_statistics(sessions: &[Session]) -> Result<StatisticsResult, ApiError> {
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
    let current_year = Utc::now().date_naive().year();
    let yearly_streak = calculate_period_streak(sessions, |date| date.year() == current_year);

    // Calculate monthly streak using the utility function
    let current_month = Utc::now().date_naive().month();
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
async fn get_weekly_trend(db: web::Data<Arc<Db>>) -> Result<impl Responder, ApiError> {
    let (sessions, _current_date, _total_points) = fetch_sessions(&db)?;
    let weekly_trend = calculate_trend(&sessions, "week");

    Ok(HttpResponse::Ok().json(weekly_trend))
}

async fn get_achievements(db: web::Data<Arc<Db>>) -> Result<impl Responder, ApiError> {
    let (sessions, _current_date, _total_points) = fetch_sessions(&db)?;
    let (overall_streak, _) = calculate_streak_and_points(&sessions)?;

    let achievements = if overall_streak >= 7 {
        vec!["7-day streak".to_string()]
    } else {
        vec![]
    };

    Ok(HttpResponse::Ok().json(AchievementsResponse { achievements }))
}

async fn get_streaks(db: web::Data<Arc<Db>>) -> Result<impl Responder, ApiError> {
    let (sessions, _current_date, _total_points) = fetch_sessions(&db)?;
    let (overall_streak, _) = calculate_streak_and_points(&sessions)?;

    let current_year = Utc::now().date_naive().year();
    let yearly_streak = calculate_period_streak(&sessions, |date| date.year() == current_year);

    let current_month = Utc::now().date_naive().month();
    let monthly_streak = calculate_period_streak(&sessions, |date| date.month() == current_month);

    Ok(HttpResponse::Ok().json(StreaksResponse {
        overall_streak,
        yearly_streak,
        monthly_streak,
    }))
}

async fn get_streak_bonuses(db: web::Data<Arc<Db>>) -> Result<impl Responder, ApiError> {
    let (sessions, _current_date, _total_points) = fetch_sessions(&db)?;
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

async fn get_overall_statistics(db: web::Data<Arc<Db>>) -> Result<impl Responder, ApiError> {
    let (sessions, current_date, total_points) = fetch_sessions(&db)?;
    let (weekly_trend, achievements, overall_streak, yearly_streak, monthly_streak) =
        calculate_statistics(&sessions)?;

    Ok(HttpResponse::Ok().json(StatisticsResponse {
        current_date, // Use the current_date from fetch_sessions
        streak: overall_streak,
        total_points, // Already calculated total_points
        weekly_trend,
        achievements,
        yearly_streak,
        monthly_streak,
    }))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok(); // Load environment variables

    // Initialize the Sled embedded database
    let db = Arc::new(sled::open("my_db").expect("Failed to open sled database"));

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db.clone())) // Share sled db across routes
            .route("/api/time", web::get().to(get_time)) // Fetch current time and streaks
            .route("/api/log_session", web::post().to(create_session)) // Log a new session
            .route(
                "/api/statistics/weekly_trend",
                web::get().to(get_weekly_trend),
            ) // Fetch weekly trends
            .route(
                "/api/statistics/achievements",
                web::get().to(get_achievements),
            ) // Fetch achievements
            .route("/api/statistics/streaks", web::get().to(get_streaks)) // Fetch streak details
            .route(
                "/api/statistics/overall",
                web::get().to(get_overall_statistics),
            ) // Fetch overall statistics
            .route("/api/bonuses/streaks", web::get().to(get_streak_bonuses)) // Fetch streak bonuses
    })
        .bind("127.0.0.1:8080")?
        .run()
        .await
}

/// tests

#[cfg(test)]
fn sample_sessions() -> Vec<Session> {
    vec![
        Session {
            date: "2023-10-01".to_string(),
            session_type: "1-hour".to_string(),
        },
        Session {
            date: "2023-10-02".to_string(),
            session_type: "2-hours".to_string(),
        },
        Session {
            date: "2023-10-03".to_string(),
            session_type: "3-hours".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test;

    // Update the existing test for calculating streak and points
    #[test]
    async fn test_calculate_streak_and_points() {
        let sessions = sample_sessions();
        let (streak, total_points) = calculate_streak_and_points(&sessions).unwrap();
        assert_eq!(streak, 3);
        assert_eq!(total_points, 36); // 10 + 12 + 14
    }

    // Test for calculate_statistics function with updates for bonuses and streaks
    #[test]
    async fn test_calculate_statistics() {
        let sessions = sample_sessions();

        // Calculate statistics with the function from the code
        let (weekly_trend, achievements, overall_streak, yearly_streak, monthly_streak) =
            calculate_statistics(&sessions).unwrap();

        // Ensure the weekly trends are correctly calculated
        assert_eq!(weekly_trend.len(), 1); // One weekly trend entry
        assert_eq!(weekly_trend[0].points, 36); // Points for the week

        // Check achievements (since there's no 7-day streak in this example)
        assert!(achievements.is_empty());

        // Check the overall streak
        assert_eq!(overall_streak, 3); // Expect a streak of 3

        // Check yearly and monthly streaks
        assert_eq!(yearly_streak, 3); // Expect a streak of 3
        assert_eq!(monthly_streak, 3); // Expect a streak of 3
    }

    // Tests for parsing valid and invalid dates
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
}

#[cfg(test)]
mod weekly_points_tests {
    use super::*;

    #[test]
    fn test_calculate_weekly_streak_bonuses() {
        let sessions = sample_sessions();

        let weekly_streak_bonuses = calculate_weekly_streak_bonus(&sessions);

        // Assert that we have one streak bonus
        assert_eq!(weekly_streak_bonuses.len(), 1);

        // Check the streak length
        assert_eq!(weekly_streak_bonuses[0].streak_length, 3);

        // Check the week start date
        assert_eq!(weekly_streak_bonuses[0].week_start, "2023-10-02");
    }
}

#[cfg(test)]
mod streaks_and_bonus_tests {
    use super::*;

    #[test]
    fn test_calculate_streaks_and_bonus_points() {
        let sessions = sample_sessions();
        let (streak, total_points) = calculate_streak_and_points(&sessions).unwrap();
        assert_eq!(streak, 3);
        assert_eq!(total_points, 36); // 10 + 12 + 14
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use actix_web::{test, App};

    #[actix_rt::test]
    async fn test_get_time_endpoint() {
        let _ = std::fs::remove_dir_all("test_db"); // Remove the existing test database before starting the test
        let db = Arc::new(sled::open("test_db").expect("Failed to open sled database"));

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(db.clone()))
                .route("/api/time", web::get().to(get_time)),
        )
            .await;

        let req = test::TestRequest::get().uri("/api/time").to_request();
        let resp: TimeResponse = test::call_and_read_body_json(&app, req).await;

        assert!(!resp.current_time.is_empty());
    }

    #[actix_rt::test]
    async fn test_get_streak_bonuses_endpoint() {
        let _ = std::fs::remove_dir_all("test_db"); // Clean up the test database
        let db = Arc::new(sled::open("test_db").expect("Failed to open sled database"));

        // Add some sample data to the database
        let sample_data = sample_sessions();
        for session in sample_data {
            db.insert(session.date.clone(), serde_json::to_vec(&session).unwrap())
                .expect("Failed to insert sample data");
        }

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(db.clone()))
                .route("/api/bonuses/streaks", web::get().to(get_streak_bonuses)),
        )
            .await;

        let req = test::TestRequest::get().uri("/api/bonuses/streaks").to_request();
        let resp: Vec<StreakBonusResponse> = test::call_and_read_body_json(&app, req).await;

        assert_eq!(resp.len(), 1);
        assert_eq!(resp[0].streak_length, 3);
        assert_eq!(resp[0].week_start, "2023-09-25");
    }
}
