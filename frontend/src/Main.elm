module Main exposing (..)

import Browser
import Http
import Html exposing (Html, button, div, input, text)
import Html.Attributes exposing (type_)
import Html.Events exposing (onClick, onInput)
import Json.Decode exposing (Decoder, field, int, map3, string)
import Json.Encode


-- MODEL

type alias Model =
    { currentTime : String
    , streak : Int
    , totalPoints : Int
    , selectedDate : String
    , selectedSession : String
    }

init : () -> ( Model, Cmd Msg )
init _ =
    ( { currentTime = "Fetching..."
      , streak = 0
      , totalPoints = 0
      , selectedDate = ""
      , selectedSession = "1-hour"
      }
    , fetchCurrentTime
    )


-- HTTP REQUESTS

fetchCurrentTime : Cmd Msg
fetchCurrentTime =
    Http.get
        { url = "http://127.0.0.1:8080/api/time"
        , expect = Http.expectJson GotTime timeResponseDecoder
        }

timeResponseDecoder : Decoder TimeResponse
timeResponseDecoder =
    map3 TimeResponse
        (field "current_time" string)
        (field "streak" int)
        (field "total_points" int)

logSession : String -> String -> Cmd Msg
logSession date sessionType =
    Http.post
        { url = "http://127.0.0.1:8080/api/log_session"
        , body = Http.jsonBody (logSessionEncoder date sessionType)
        , expect = Http.expectJson GotTime timeResponseDecoder
        }

logSessionEncoder : String -> String -> Json.Encode.Value
logSessionEncoder date sessionType =
    Json.Encode.object
        [ ( "date", Json.Encode.string date )
        , ( "session_type", Json.Encode.string sessionType )
        ]


-- MESSAGES

type Msg
    = GotTime (Result Http.Error TimeResponse)
    | SelectDate String
    | SelectSession String
    | LogSession


type alias TimeResponse =
    { current_time : String
    , streak : Int
    , total_points : Int
    }


-- UPDATE

update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        GotTime (Ok response) ->
            ( { model
                | currentTime = response.current_time
                , streak = response.streak
                , totalPoints = response.total_points
              }
            , Cmd.none
            )

        GotTime (Err _) ->
            ( { model | currentTime = "Failed to fetch time" }, Cmd.none )

        SelectDate date ->
            ( { model | selectedDate = date }, Cmd.none )

        SelectSession sessionType ->
            ( { model | selectedSession = sessionType }, Cmd.none )

        LogSession ->
            ( model, logSession model.selectedDate model.selectedSession )


-- VIEW

view : Model -> Html Msg
view model =
    div []
        [ div [] [ text ("Current Date: " ++ model.currentTime) ]
        , div [] [ text ("Streak: " ++ String.fromInt model.streak ++ " days") ]
        , div [] [ text ("Total Points: " ++ String.fromInt model.totalPoints) ]
        , div []
            [ input [ type_ "date", onInput SelectDate ] []
            , input [ type_ "text", onInput SelectSession ] []
            ]
        , button [ onClick LogSession ] [ text "Log Session" ]
        ]

-- MAIN

main =
    Browser.element { init = init, update = update, view = view, subscriptions = \_ -> Sub.none }
