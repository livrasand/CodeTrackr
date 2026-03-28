use axum::{
    extract::State,
    http::{StatusCode, header},
    response::{Json, Response},
};
use serde_json::{json, Value};
use uuid::Uuid;
use rand::Rng;
use sqlx::Row;
use crate::{AppState, models::User};

/// Generate anonymous username (random adjective + animal combination)
fn generate_anonymous_username() -> String {
    let adjectives = vec![
        "silent", "swift", "clever", "bright", "calm", "brave", "wise", "kind",
        "quiet", "noble", "gentle", "bold", "smart", "cool", "warm", "soft",
        "fierce", "grumpy", "lazy", "angry", "happy", "silly", "sneaky", "stupid",
        "tall", "short", "strong", "weak", "powerful", "slow", "fast", "old", "young",
        "tiny", "huge", "giant", "micro", "mega", "ultra", "super", "hyper", "mini", "maxi",
        "red", "blue", "green", "yellow", "purple", "orange", "pink", "brown", "black", "white",
        "golden", "silver", "bronze", "crystal", "rainbow", "neon", "glowing", "shiny", "dark", "light",
        "cosmic", "stellar", "galactic", "solar", "lunar", "planetary", "meteoric", "comet", "asteroid", "nebula",
        "arctic", "tropical", "desert", "oceanic", "mountain", "forest", "river", "lake", "volcanic", "glacial",
        "electric", "magnetic", "atomic", "nuclear", "quantum", "digital", "cyber", "tech", "nano", "giga",
        "ancient", "modern", "future", "past", "eternal", "temporal", "infinite", "finite", "momentary", "lasting",
        "dreamy", "mystic", "magic", "enchanted", "cursed", "blessed", "divine", "demonic", "angelic", "mythic",
        "spicy", "sweet", "sour", "bitter", "salty", "fresh", "rotten", "delicious", "tasty", "flavorful",
        "musical", "rhythmic", "melodic", "harmonic", "symphonic", "jazzy", "rock", "pop", "classical", "folk",
        "sporty", "athletic", "agile", "flexible", "stiff", "rigid", "elastic", "bouncy", "springy", "dynamic",
        "frozen", "boiling", "steaming", "chilly", "freezing", "scorching", "burning", "heated", "cold", "hot",
        "wooden", "metal", "plastic", "glass", "stone", "iron", "steel", "copper", "bronze", "titanium",
        "velvet", "silk", "cotton", "wool", "leather", "fur", "feather", "scale", "shell", "crystal",
        "acidic", "alkaline", "neutral", "basic", "chemical", "organic", "synthetic", "natural", "artificial", "pure",
        "urban", "rural", "wild", "tame", "domestic", "feral", "civilized", "primitive", "advanced", "simple",
        "chaotic", "orderly", "random", "structured", "organized", "messy", "clean", "dirty", "neat", "tidy",
        "sharp", "blunt", "pointed", "rounded", "flat", "curved", "straight", "bent", "twisted", "spiral",
        "liquid", "solid", "gas", "plasma", "frozen", "molten", "vapor", "mist", "fog", "cloud",
        "alpha", "beta", "gamma", "delta", "omega", "prime", "ultimate", "final", "first", "last",
        "eastern", "western", "northern", "southern", "tropical", "polar", "equatorial", "coastal", "inland", "island",
        "spring", "summer", "autumn", "winter", "seasonal", "yearly", "monthly", "weekly", "daily", "hourly",
        "royal", "imperial", "noble", "peasant", "common", "rare", "unique", "special", "ordinary", "normal",
        "toxic", "poisonous", "venomous", "harmless", "safe", "dangerous", "risky", "secure", "protected", "vulnerable",
        "hungry", "thirsty", "full", "empty", "satisfied", "starving", "quenched", "dehydrated", "bloated", "lean",
        "awake", "asleep", "dreaming", "conscious", "unconscious", "alert", "drowsy", "energized", "tired", "exhausted",
        "rich", "poor", "wealthy", "broke", "expensive", "cheap", "valuable", "worthless", "precious", "common",
        "famous", "unknown", "celebrated", "forgotten", "notorious", "infamous", "legendary", "mythical", "historic", "modern",
        "legal", "illegal", "official", "unofficial", "authorized", "forbidden", "permitted", "banned", "allowed", "restricted",
        "healthy", "sick", "ill", "well", "fit", "unfit", "strong", "frail", "robust", "delicate",
        "open", "closed", "locked", "unlocked", "sealed", "broken", "intact", "shattered", "complete", "partial",
        "deep", "shallow", "profound", "superficial", "meaningful", "meaningless", "significant", "trivial", "important", "minor",
        "true", "false", "real", "fake", "genuine", "artificial", "authentic", "counterfeit", "original", "copy",
        "left", "right", "center", "forward", "backward", "up", "down", "north", "south", "east", "west",
        "early", "late", "punctual", "tardy", "timely", "delayed", "prompt", "slow", "quick", "instant",
        "big", "small", "large", "little", "massive", "tiny", "enormous", "minute", "vast", "compact",
        "good", "bad", "evil", "virtuous", "wicked", "righteous", "sinful", "moral", "immoral", "ethical",
        "new", "old", "fresh", "stale", "recent", "ancient", "modern", "outdated", "current", "obsolete",
        "easy", "hard", "simple", "complex", "difficult", "challenging", "effortless", "demanding", "basic", "advanced",
        "light", "heavy", "weightless", "massive", "feather", "lead", "airy", "dense", "thick", "thin",
        "loud", "quiet", "noisy", "silent", "soft", "deafening", "whisper", "shout", "muted", "amplified",
        "clean", "dirty", "pure", "filthy", "spotless", "messy", "tidy", "disorganized", "neat", "chaotic",
        "dry", "wet", "moist", "soaked", "damp", "drenched", "arid", "humid", "dehydrated", "saturated",
        "hot", "cold", "warm", "cool", "freezing", "boiling", "scorching", "icy", "mild", "extreme",
        "fast", "slow", "quick", "sluggish", "rapid", "gradual", "instant", "delayed", "swift", "leisurely",
        "high", "low", "tall", "short", "elevated", "grounded", "lofty", "deep", "superior", "inferior",
        "near", "far", "close", "distant", "adjacent", "remote", "local", "foreign", "immediate", "removed",
        "hard", "soft", "firm", "gentle", "rigid", "flexible", "stiff", "pliable", "solid", "liquid",
        "bright", "dark", "dim", "vivid", "pale", "intense", "faded", "radiant", "gloomy", "luminous",
        "smooth", "rough", "slick", "coarse", "silky", "gritty", "polished", "raw", "refined", "unrefined",
        "straight", "curved", "bent", "twisted", "direct", "indirect", "level", "tilted", "vertical", "horizontal",
        "thick", "thin", "wide", "narrow", "broad", "slim", "expansive", "compact", "spacious", "cramped",
        "young", "old", "mature", "immature", "aged", "youthful", "elderly", "fresh", "seasoned", "novice",
        "first", "last", "initial", "final", "primary", "secondary", "main", "minor", "chief", "assistant",
        "full", "empty", "complete", "partial", "whole", "broken", "intact", "damaged", "perfect", "flawed",
        "alive", "dead", "living", "lifeless", "active", "inactive", "dynamic", "static", "vibrant", "dull",
        "sharp", "dull", "keen", "blunt", "acute", "obtuse", "precise", "vague", "clear", "unclear",
        "rich", "poor", "wealthy", "impoverished", "abundant", "scarce", "plentiful", "rare", "common", "unusual",
        "strong", "weak", "powerful", "feeble", "mighty", "frail", "robust", "delicate", "sturdy", "fragile",
        "free", "bound", "liberated", "trapped", "independent", "dependent", "autonomous", "controlled", "unrestrained", "confined",
        "wild", "tame", "feral", "domestic", "untamed", "civilized", "savage", "gentle", "barbaric", "refined",
        "pure", "impure", "clean", "contaminated", "unmixed", "polluted", "clear", "murky", "transparent", "opaque",
        "wise", "foolish", "intelligent", "stupid", "smart", "dumb", "clever", "clumsy", "bright", "dim",
        "brave", "cowardly", "courageous", "fearful", "bold", "timid", "heroic", "villainous", "valiant", "weak",
        "kind", "cruel", "gentle", "harsh", "compassionate", "ruthless", "merciful", "merciless", "tender", "tough",
        "honest", "dishonest", "truthful", "deceitful", "sincere", "insincere", "genuine", "fake", "authentic", "phony",
        "loyal", "disloyal", "faithful", "unfaithful", "true", "false", "devoted", "treacherous", "committed", "uncommitted",
        "patient", "impatient", "tolerant", "intolerant", "calm", "agitated", "serene", "restless", "peaceful", "turbulent",
        "humble", "proud", "modest", "arrogant", "meek", "haughty", "unassuming", "conceited", "down-to-earth", "pretentious",
        "generous", "stingy", "giving", "selfish", "charitable", "greedy", "benevolent", "malevolent", "philanthropic", "miserly",
        "optimistic", "pessimistic", "hopeful", "hopeless", "positive", "negative", "cheerful", "gloomy", "bright", "dark",
        "confident", "insecure", "sure", "doubtful", "certain", "uncertain", "assured", "hesitant", "bold", "timid",
        "creative", "uncreative", "imaginative", "dull", "innovative", "traditional", "original", "derivative", "artistic", "scientific",
        "curious", "indifferent", "inquisitive", "apathetic", "interested", "bored", "engaged", "disengaged", "fascinated", "uninterested",
        "ambitious", "unambitious", "driven", "aimless", "motivated", "unmotivated", "determined", "undetermined", "focused", "unfocused",
        "disciplined", "undisciplined", "controlled", "uncontrolled", "restrained", "unrestrained", "regulated", "unregulated", "orderly", "disorderly",
        "responsible", "irresponsible", "accountable", "unaccountable", "reliable", "unreliable", "dependable", "undependable", "trustworthy", "untrustworthy",
        "flexible", "rigid", "adaptable", "stubborn", "versatile", "inflexible", "pliable", "unyielding", "malleable", "brittle",
        "organized", "disorganized", "methodical", "chaotic", "systematic", "random", "structured", "unstructured", "planned", "spontaneous",
        "efficient", "inefficient", "productive", "unproductive", "effective", "ineffective", "successful", "unsuccessful", "competent", "incompetent",
        "friendly", "unfriendly", "sociable", "antisocial", "outgoing", "withdrawn", "extroverted", "introverted", "approachable", "unapproachable",
        "polite", "rude", "courteous", "discourteous", "respectful", "disrespectful", "considerate", "inconsiderate", "thoughtful", "thoughtless",
        "helpful", "unhelpful", "supportive", "unsupportive", "cooperative", "uncooperative", "collaborative", "uncooperative", "teamwork", "solo",
        "punctual", "late", "timely", "untimely", "prompt", "delayed", "early", "overdue", "scheduled", "unscheduled",
        "neat", "messy", "tidy", "untidy", "orderly", "disorderly", "clean", "unclean", "organized", "disorganized",
        "careful", "careless", "cautious", "reckless", "meticulous", "negligent", "thorough", "superficial", "detailed", "vague",
        "patient", "impatient", "enduring", "impatient", "persistent", "giving-up", "persevering", "quitting", "determined", "undetermined",
    ];
    let animals = vec![
        "fox", "wolf", "bear", "eagle", "owl", "hawk", "lion", "tiger",
        "deer", "rabbit", "turtle", "dolphin", "whale", "horse", "zebra", "panther",
        "cat", "dog", "elephant", "kangaroo", "monkey", "penguin", "snake", "shark",
        "chicken", "giraffe", "octopus", "panda", "rhino", "koala", "platypus",
        "raccoon", "seal", "squirrel", "turtle", "yak",
        "antelope", "badger", "bat", "beaver", "bison", "boar", "buffalo", "camel",
        "caribou", "cheetah", "cobra", "coyote", "crocodile", "crow", "dingo", "elk",
        "emu", "ferret", "flamingo", "frog", "gecko", "gerbil", "goat", "gorilla",
        "hamster", "hedgehog", "hippo", "hyena", "iguana", "jackal", "jaguar", "lemur",
        "leopard", "llama", "lynx", "manatee", "meerkat", "moose", "narwhal", "ocelot",
        "orangutan", "ostrich", "otter", "pelican", "porcupine", "possum", "quail", "rabbit",
        "rat", "reindeer", "salamander", "scorpion", "seahorse", "skunk", "sloth", "sparrow",
        "stingray", "stork", "swan", "tapir", "tarantula", "toucan", "vulture", "wallaby",
        "warthog", "wombat", "woodpecker", "zebra", "aardvark", "albatross", "alligator", "alpaca",
        "anchovy", "angelfish", "ant", "anteater", "armadillo", "axolotl", "baboon", "barracuda",
        "basilisk", "bass", "bat", "bear", "bee", "beetle", "bison", "boar",
        "bobcat", "buffalo", "butterfly", "camel", "canary", "capybara", "cardinal", "carp",
        "catfish", "caterpillar", "catshark", "cattle", "centipede", "chameleon", "cheetah", "chickadee",
        "chicken", "chimpanzee", "chinchilla", "chipmunk", "clownfish", "cobra", "cockroach", "cod",
        "condor", "coral", "cougar", "cow", "coyote", "crab", "crane", "crayfish",
        "crow", "cuckoo", "cicada", "damselfly", "deer", "dingo", "dinosaur", "dog",
        "dolphin", "donkey", "dove", "dragon", "dragonfly", "duck", "eagle", "earwig",
        "echidna", "eel", "egret", "elephant", "elk", "emu", "ermine", "falcon",
        "ferret", "finch", "firefly", "fish", "flamingo", "flea", "fly", "flyingfish",
        "fox", "frog", "gazelle", "gecko", "gerbil", "gibbon", "goat", "goldfish",
        "goose", "gopher", "gorilla", "grasshopper", "grouse", "guanaco", "guinea", "gull",
        "hamster", "hare", "harrier", "hawk", "hedgehog", "heron", "herring", "hippopotamus",
        "hornet", "horse", "hoverfly", "hummingbird", "hyena", "iguana", "impala", "jackal",
        "jaguar", "jay", "jellyfish", "kangaroo", "kingfisher", "kite", "kiwi", "koala",
        "koi", "komodo", "krill", "ladybug", "lamprey", "lark", "lemming", "lemur",
        "leopard", "lion", "lizard", "llama", "lobster", "locust", "loon", "lynx",
        "macaw", "magpie", "mallard", "manatee", "mandrill", "mantis", "marmot", "marsupial",
        "meerkat", "mole", "mongoose", "monkey", "moose", "mosquito", "moth", "mouse",
        "mule", "narwhal", "newt", "nightingale", "octopus", "opossum", "orangutan", "ostrich",
        "otter", "owl", "ox", "oyster", "panda", "panther", "parakeet", "parrot",
        "partridge", "peacock", "pelican", "penguin", "pheasant", "pig", "pigeon", "pike",
        "pillbug", "piranha", "planarian", "platypus", "pony", "porcupine", "porpoise", "possum",
        "prawn", "primate", "puffin", "puma", "python", "quail", "quokka", "rabbit",
        "raccoon", "rat", "rattlesnake", "raven", "reindeer", "rhinoceros", "roadrunner", "robin",
        "rodent", "rooster", "roundworm", "sailfish", "salamander", "salmon", "sawfish", "scallop",
        "scorpion", "sea", "seahorse", "seal", "shark", "sheep", "shrew", "shrimp",
        "silkworm", "skink", "skunk", "sloth", "slug", "smelt", "snail", "snake",
        "sparrow", "spider", "spoonbill", "squid", "squirrel", "starfish", "stingray", "stinkbug",
        "stork", "stoat", "sturgeon", "swallow", "swan", "swordfish", "tamarin", "tapir",
        "tarantula", "tarsier", "termite", "tern", "thrush", "tick", "tiger", "tigon",
        "toad", "tortoise", "toucan", "trout", "tuna", "turkey", "turtle", "tyrannosaurus",
        "unicorn", "urial", "vampire", "viper", "vulture", "wallaby", "walrus", "warthog",
        "wasp", "water", "weasel", "wildebeest", "wolf", "wolverine", "wombat", "woodpecker",
        "worm", "wren", "yak", "zebra", "zebu", "zorilla", "zorse", "abyssinian",
        "albatross", "angelfish", "anole", "antbird", "antelope", "archerfish", "avocet", "axolotl",
        "baboon", "badger", "bandicoot", "banteng", "barbet", "barracuda", "basset", "bat",
        "beagle", "bear", "beaver", "binturong", "bird", "bison", "bloodhound", "boar",
        "bobcat", "bongo", "bonobo", "booby", "borzoi", "boston", "boubou", "bowl",
        "boxer", "budgerigar", "buffalo", "bulldog", "bullfrog", "bunny", "bustard", "butterfly",
        "caiman", "camel", "canary", "capybara", "cardinal", "caribou", "carp", "cat",
        "caterpillar", "catfish", "cattle", "centipede", "chameleon", "chamois", "cheetah", "chicken",
        "chihuahua", "chimpanzee", "chinchilla", "chipmunk", "cichlid", "cicada", "civet", "clownfish",
        "cobra", "cockroach", "cod", "collie", "condor", "conure", "cormorant", "cougar",
        "cow", "coyote", "crab", "crane", "crawdad", "crayfish", "cricket", "crocodile",
        "crow", "cuckoo", "cuscus", "cuttlefish", "dachshund", "dalmatian", "deer", "dingo",
        "dinosaur", "dog", "dolphin", "donkey", "dove", "dragon", "dragonfly", "duck",
        "dugong", "eagle", "earwig", "echidna", "eel", "egret", "elephant", "elk",
        "emu", "ermine", "falcon", "ferret", "finch", "firefly", "fish", "flamingo",
        "flea", "fly", "flyingfish", "fowl", "fox", "frog", "gallinule", "gamefowl",
        "gazelle", "gecko", "gerbil", "gharial", "gibbon", "giraffe", "goat", "goldfish",
        "goose", "gopher", "gorilla", "grasshopper", "grouse", "guan", "guanaco", "guinea",
        "gull", "hamster", "hare", "harrier", "hawk", "hedgehog", "heron", "herring",
        "hippopotamus", "hookworm", "hornet", "horse", "hoverfly", "hummingbird", "hyena", "ibis",
        "iguana", "impala", "indri", "insect", "jackal", "jaguar", "jay", "jellyfish",
        "kakapo", "kangaroo", "kingfisher", "kite", "kiwi", "koala", "koi", "komodo",
        "kudu", "labrador", "ladybug", "lamprey", "lark", "lemming", "lemur", "leopard",
        "lion", "lizard", "llama", "lobster", "locust", "loon", "lynx", "macaw",
        "magpie", "mallard", "manatee", "mandrill", "mantis", "marmoset", "marmot", "marsupial",
        "meerkat", "megalodon", "megalosaurus", "mole", "mongoose", "monitor", "monkey", "moose",
        "mosquito", "moth", "mouse", "mule", "narwhal", "newt", "nightingale", "numbat",
        "ocelot", "octopus", "opossum", "orangutan", "ostrich", "otter", "owl", "ox",
        "oyster", "panda", "panther", "parakeet", "parrot", "partridge", "peacock", "pelican",
        "penguin", "pheasant", "pig", "pigeon", "pike", "pilot", "pinniped", "piranha",
        "pizzly", "planarian", "platypus", "pointer", "pony", "porcupine", "porpoise", "possum",
        "prairie", "prawn", "primate", "puffin", "puma", "puma", "python", "quail",
        "quokka", "rabbit", "raccoon", "rat", "rattlesnake", "raven", "reindeer", "rhinoceros",
        "roadrunner", "robin", "rodent", "rooster", "roundworm", "sailfish", "salamander", "salmon",
        "sawfish", "scallop", "scorpion", "seahorse", "seal", "shark", "sheep", "shrew",
        "shrimp", "silkworm", "skink", "skunk", "sloth", "slug", "smelt", "snail",
        "snake", "sparrow", "spider", "spoonbill", "squid", "squirrel", "starfish", "stingray",
        "stinkbug", "stork", "stoat", "sturgeon", "swallow", "swan", "swordfish", "tamarin",
        "tapir", "tarantula", "tarsier", "termite", "tern", "thrush", "tick", "tiger",
        "tigon", "toad", "tortoise", "toucan", "trout", "tuna", "turkey", "turtle",
        "tyrannosaurus", "unicorn", "urial", "vampire", "viper", "vulture", "wallaby", "walrus",
        "warthog", "wasp", "weasel", "wildebeest", "wolf", "wolverine", "wombat", "woodpecker",
        "worm", "wren", "yak", "zebra", "zebu", "zorilla", "zorse",
    ];
    
    let mut rng = rand::thread_rng();
    let adjective = adjectives[rng.gen_range(0..adjectives.len())];
    let animal = animals[rng.gen_range(0..animals.len())];
    let number = rng.gen_range(100..9999);
    
    format!("{}-{}-{}", adjective, animal, number)
}

/// Generate a random 16-digit account number (Mullvad-style)
fn generate_account_number() -> String {
    let mut rng = rand::thread_rng();
    (0..16).map(|_| rng.gen_range(0..10).to_string()).collect()
}

/// Create real JWT token for anonymous user
fn create_anonymous_jwt(user_id: &str, jwt_secret: &str) -> Result<String, String> {
    // Use the standard claims shape so AuthenticatedUser can verify it.
    let expires_in_seconds = 24 * 60 * 60; // 24 hours
    crate::auth::create_jwt(user_id, jwt_secret, crate::models::TokenType::Access, expires_in_seconds)
        .map_err(|e| format!("Failed to create JWT: {}", e))
}

/// Create anonymous account - generates account number and returns JWT
pub async fn create_anonymous_account(
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let account_number = generate_account_number();
    
    // Check for collision (extremely unlikely but we handle it)
    let existing = sqlx::query("SELECT id FROM users WHERE account_number = $1")
        .bind(&account_number)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| {
            tracing::error!("DB error checking account number collision: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?;

    if existing.is_some() {
        // Extremely unlikely collision — regenerate and retry inline
        let account_number = generate_account_number();
        let existing2 = sqlx::query("SELECT id FROM users WHERE account_number = $1")
            .bind(&account_number)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(|e| {
                tracing::error!("DB error checking account number collision (retry): {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "Database error"})),
                )
            })?;
        if existing2.is_some() {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Could not generate unique account number"})),
            ));
        }
        let _ = account_number; // will shadow below
    }

    // Create user with anonymous account
    let user_id = Uuid::new_v4();
    // Usar el username aleatorio generado, no el número de cuenta
    let username = generate_anonymous_username();
    
    let user_row = sqlx::query(
        r#"
        INSERT INTO users (
            id, username, account_number, plan, is_public, is_anonymous,
            timezone, created_at, updated_at
        ) VALUES (
            $1, $2, $3, 'free', true, true, 'UTC', NOW(), NOW()
        ) RETURNING id, username, display_name, email, avatar_url, github_id,
        gitlab_id, account_number, is_anonymous, plan, stripe_customer_id,
        stripe_subscription_id, plan_expires_at, is_public, is_admin, bio,
        website, profile_show_languages, profile_show_projects,
        profile_show_activity, profile_show_plugins, profile_show_streak,
        available_for_hire, country, timezone, created_at, updated_at
        "#
    )
    .bind(user_id)
    .bind(&username)
    .bind(&account_number)
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to create anonymous user: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to create account"})),
        )
    })?;

    // Convert to User struct
    let user = User {
        id: user_row.get("id"),
        username: user_row.get("username"),
        display_name: user_row.get("display_name"),
        email: user_row.get("email"),
        avatar_url: user_row.get("avatar_url"),
        github_id: user_row.get("github_id"),
        gitlab_id: user_row.get("gitlab_id"),
        account_number: user_row.get("account_number"),
        is_anonymous: user_row.get("is_anonymous"),
        plan: user_row.get("plan"),
        stripe_customer_id: user_row.get("stripe_customer_id"),
        stripe_subscription_id: user_row.get("stripe_subscription_id"),
        plan_expires_at: user_row.get("plan_expires_at"),
        is_public: user_row.get("is_public"),
        is_admin: user_row.get("is_admin"),
        bio: user_row.get("bio"),
        website: user_row.get("website"),
        profile_show_languages: user_row.get("profile_show_languages"),
        profile_show_projects: user_row.get("profile_show_projects"),
        profile_show_activity: user_row.get("profile_show_activity"),
        profile_show_plugins: user_row.get("profile_show_plugins"),
        profile_show_streak: user_row.get("profile_show_streak"),
        available_for_hire: user_row.get("available_for_hire"),
        show_in_leaderboard: user_row.get("show_in_leaderboard"),
        country: user_row.get("country"),
        timezone: user_row.get("timezone"),
        created_at: user_row.get("created_at"),
        updated_at: user_row.get("updated_at"),
    };

    // Create real JWT access token
    let access_token = create_anonymous_jwt(&user_id.to_string(), &state.config.jwt_secret)
        .map_err(|e| {
            tracing::error!("Access token creation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication failed"})),
            )
        })?;

    // Create refresh token if available
    let mut response_data = json!({
        "access_token": access_token,
        "user": {
            "id": user.id,
            "username": user.username,
            "account_number": user.account_number,
            "plan": user.plan,
            "created_at": user.created_at
        },
        "message": "Account created successfully. Save your account number: you'll need it to login."
    });

    // Create refresh token using real service
    match create_anonymous_refresh_token(user_id, &account_number, &state).await {
        Ok(refresh_response) => {
            response_data["refresh_token"] = json!(refresh_response.refresh_token);
        }
        Err(e) => {
            tracing::warn!("Failed to create refresh token: {}", e);
            // Continue without refresh token - access token is still valid
        }
    }

    let response = Json(response_data);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(response.to_string().into())
        .map_err(|e| crate::error_handling::handle_auth_error(e))?)
}

/// Login with account number - returns JWT if account exists
pub async fn login_with_account_number(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let account_number = payload.get("account_number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "account_number is required"})),
            )
        })?;

    // Validate account number format (16 digits)
    if account_number.len() != 16 || !account_number.chars().all(|c| c.is_ascii_digit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid account number format"})),
        ));
    }

    let user_row = sqlx::query(
        "SELECT id, username, display_name, email, avatar_url, github_id,
         gitlab_id, account_number, is_anonymous, plan, stripe_customer_id,
         stripe_subscription_id, plan_expires_at, is_public, is_admin, bio,
         website, profile_show_languages, profile_show_projects,
         profile_show_activity, profile_show_plugins, profile_show_streak,
         available_for_hire, country, timezone, created_at, updated_at
         FROM users WHERE account_number = $1"
    )
    .bind(account_number)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| {
        tracing::error!("DB error finding user by account number: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Database error"})),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid account number"})),
        )
    })?;

    // Convert to User struct
    let user = User {
        id: user_row.get("id"),
        username: user_row.get("username"),
        display_name: user_row.get("display_name"),
        email: user_row.get("email"),
        avatar_url: user_row.get("avatar_url"),
        github_id: user_row.get("github_id"),
        gitlab_id: user_row.get("gitlab_id"),
        account_number: user_row.get("account_number"),
        is_anonymous: user_row.get("is_anonymous"),
        plan: user_row.get("plan"),
        stripe_customer_id: user_row.get("stripe_customer_id"),
        stripe_subscription_id: user_row.get("stripe_subscription_id"),
        plan_expires_at: user_row.get("plan_expires_at"),
        is_public: user_row.get("is_public"),
        is_admin: user_row.get("is_admin"),
        bio: user_row.get("bio"),
        website: user_row.get("website"),
        profile_show_languages: user_row.get("profile_show_languages"),
        profile_show_projects: user_row.get("profile_show_projects"),
        profile_show_activity: user_row.get("profile_show_activity"),
        profile_show_plugins: user_row.get("profile_show_plugins"),
        profile_show_streak: user_row.get("profile_show_streak"),
        available_for_hire: user_row.get("available_for_hire"),
        show_in_leaderboard: user_row.get("show_in_leaderboard"),
        country: user_row.get("country"),
        timezone: user_row.get("timezone"),
        created_at: user_row.get("created_at"),
        updated_at: user_row.get("updated_at"),
    };

    // Create real JWT access token
    let access_token = create_anonymous_jwt(&user.id.to_string(), &state.config.jwt_secret)
        .map_err(|e| {
            tracing::error!("Access token creation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication failed"})),
            )
        })?;

    // Create response with optional refresh token
    let mut response_data = json!({
        "access_token": access_token,
        "user": {
            "id": user.id,
            "username": user.username,
            "account_number": user.account_number,
            "plan": user.plan,
            "created_at": user.created_at
        }
    });

    // Create refresh token using real service
    match create_anonymous_refresh_token(user.id, &user.account_number.as_ref().unwrap(), &state).await {
        Ok(refresh_response) => {
            response_data["refresh_token"] = json!(refresh_response.refresh_token);
        }
        Err(e) => {
            tracing::warn!("Failed to create refresh token: {}", e);
            // Continue without refresh token - access token is still valid
        }
    }

    let response = Json(response_data);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(response.to_string().into())
        .map_err(|e| crate::error_handling::handle_auth_error(e))?)
}

/// Verify account number exists (for frontend validation)
pub async fn verify_account_number(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let account_number = payload.get("account_number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "account_number is required"})),
            )
        })?;

    // Validate format
    if account_number.len() != 16 || !account_number.chars().all(|c| c.is_ascii_digit()) {
        return Ok(Json(json!({
            "valid": false,
            "message": "Invalid account number format"
        })));
    }

    // Check if exists
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE account_number = $1)")
        .bind(account_number)
        .fetch_one(&state.db.pool)
        .await
        .map_err(|e| {
            tracing::error!("DB error verifying account number: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?;

    Ok(Json(json!({
        "valid": exists,
        "message": if exists {
            "Account number found"
        } else {
            "Account number not found"
        }
    })))
}

/// Create refresh token for anonymous user using real service
async fn create_anonymous_refresh_token(
    user_id: Uuid,
    account_number: &str,
    state: &AppState,
) -> Result<crate::models::RefreshTokenResponse, Box<dyn std::error::Error>> {
    use crate::models::CreateRefreshTokenRequest;
    use crate::services::refresh_tokens::RefreshTokenService;
    
    let refresh_request = CreateRefreshTokenRequest {
        device_id: format!("anonymous-{}", account_number),
        device_info: Some(serde_json::json!({
            "type": "anonymous",
            "account_number": account_number,
            "created_at": chrono::Utc::now().to_rfc3339()
        })),
    };
    
    RefreshTokenService::create_token(
        user_id,
        refresh_request,
        None, // IP address
        None, // User agent
        state,
    ).await.map_err(|e| {
        tracing::error!("Failed to create refresh token: {}", e);
        e.into()
    })
}
