use crate::error::Result;
use crate::http::{api_url, HttpClient};
use crate::model::{
    Avatar, Ban, Channel, CreateMessage, ForumTag, Guild, Member, Message, Relationship, Role,
    SupplementalMember, SupplementalMessageRequest, User, UserProfile,
};
use serde_json::{json, Value};

/// Manager for user-related endpoints.
#[derive(Debug, Clone, Copy, Default)]
pub struct UsersManager;

impl UsersManager {
    /// Fetches the current user (`/users/@me`). SEE: <https://docs.discord.food/resources/user#get-current-user>
    pub async fn me(&self, http: &HttpClient) -> Result<User> {
        let response = http.get(api_url("/users/@me")).await?;
        let user = serde_json::from_value(response)?;
        Ok(user)
    }

    /// Modifies the current user (`PATCH /users/@me`). SEE: <https://docs.discord.food/resources/user#modify-current-user>
    pub async fn update_me<T: serde::Serialize>(&self, http: &HttpClient, data: T) -> Result<User> {
        let response = http.patch(api_url("/users/@me"), data).await?;
        let user = serde_json::from_value(response)?;
        Ok(user)
    }

    /// Fetches a user by id (`/users/{id}`). SEE: <https://docs.discord.food/resources/user#get-user>
    ///
    /// # Note
    ///  User accounts cannot fetch `/users/{id}` directly. You must provide a `bot_token`
    /// to authenticate as a bot, or use `get_profile()` instead which works for user accounts.
    pub async fn get(
        &self,
        http: &HttpClient,
        user_id: impl AsRef<str>,
        bot_token: Option<&str>,
    ) -> Result<User> {
        let client = if let Some(token) = bot_token {
            // Create temporary HTTP client with bot token
            HttpClient::new(token)?
        } else {
            // Use existing client (will likely fail for user tokens)
            http.clone()
        };

        let response = client
            .get(api_url(&format!("/users/{}", user_id.as_ref())))
            .await?;
        let user = serde_json::from_value(response)?;
        Ok(user)
    }

    /// Fetches a user profile by id (`/users/{id}/profile`).
    pub async fn get_profile(
        &self,
        http: &HttpClient,
        user_id: impl AsRef<str>,
    ) -> Result<UserProfile> {
        let response = http
            .get(api_url(&format!("/users/{}/profile", user_id.as_ref())))
            .await?;
        let profile = serde_json::from_value(response)?;
        Ok(profile)
    }

    /// Modifies the current user's profile (`PATCH /users/@me/profile`). SEE: <https://docs.discord.food/resources/user#modify-user-profile>
    pub async fn update_profile(
        &self,
        http: &HttpClient,
        data: impl serde::Serialize,
    ) -> Result<UserProfile> {
        let response = http.patch(api_url("/users/@me/profile"), data).await?;
        let profile = serde_json::from_value(response)?;
        Ok(profile)
    }

    /// Fetches the mutual relationship between the current user and another user (`GET /users/{user_id}/relationships`).
    pub async fn mutual_relationship(
        &self,
        http: &HttpClient,
        user_id: impl AsRef<str>,
    ) -> Result<Vec<User>> {
        let response = http
            .get(api_url(&format!(
                "/users/{}/relationships",
                user_id.as_ref()
            )))
            .await?;
        let relationship = serde_json::from_value(response)?;
        Ok(relationship)
    }

    /// Checks whether a unique username is available for the user to claim. SEE: <https://docs.discord.food/resources/user#get-unique-username-eligibility>
    pub async fn check_username_eligibility(
        &self,
        http: &HttpClient,
        username: impl AsRef<str>,
    ) -> Result<Value> {
        let response = http
            .post(
                api_url("/users/@me/pomelo-attempt"),
                json!({ "username": username.as_ref() }),
            )
            .await?;
        let available = serde_json::from_value(response)?;
        Ok(available)
    }

    /// Sets the current user's primary guild. Returns a user object on success. Fires a User Update Gateway event. SEE: <https://docs.discord.food/resources/user#set-guild-identity>
    pub async fn set_primary_guild(
        &self,
        http: &HttpClient,
        identity_enabled: bool,
        identity_guild_id: impl AsRef<str>,
    ) -> Result<User> {
        let response = http
            .put(
                api_url("/users/@me/clan"),
                json!({ "identity_enabled": identity_enabled, "identity_guild_id": identity_guild_id.as_ref() }),
            )
            .await?;
        let user = serde_json::from_value(response)?;
        Ok(user)
    }

    /// Get Recent Avatars of the current user (`GET /users/@me/avatars`).
    pub async fn recent_avatars(&self, http: &HttpClient) -> Result<Vec<Avatar>> {
        let response = http.get(api_url("/users/@me/avatars")).await?;
        let avatars: Vec<Avatar> = serde_json::from_value(response)?;
        Ok(avatars)
    }

    /// Delete Recent Avatar (`DELETE /users/@me/avatars/{avatar_id}`).
    pub async fn delete_recent_avatar(
        &self,
        http: &HttpClient,
        avatar_id: impl AsRef<str>,
    ) -> Result<()> {
        http.delete(api_url(&format!(
            "/users/@me/avatars/{}",
            avatar_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Joins a Hypesquad SEE: <https://docs.discord.food/resources/user#join-hypesquad-online>
    pub async fn join_hypesquad(&self, http: &HttpClient, hypesquad_house_id: u8) -> Result<()> {
        http.post(
            api_url("/users/@me/hypesquad/online"),
            json!({ "house_id": hypesquad_house_id }),
        )
        .await?;
        Ok(())
    }

    /// Leaves the Hypesquad SEE: <https://docs.discord.food/resources/user#leave-hypesquad-online>
    pub async fn leave_hypesquad(&self, http: &HttpClient) -> Result<()> {
        http.delete(api_url("/users/@me/hypesquad/online")).await?;
        Ok(())
    }
}

/// Manager for guild-related endpoints.
#[derive(Debug, Clone, Copy, Default)]
pub struct GuildsManager;

impl GuildsManager {
    /// Fetches current guild member objects for the current user (`/users/@me/guilds/{guild.id}/member`).
    pub async fn me_member(&self, http: &HttpClient, guild_id: impl AsRef<str>) -> Result<Member> {
        let response = http
            .get(api_url(&format!(
                "/users/@me/guilds/{}/member",
                guild_id.as_ref()
            )))
            .await?;
        let member = serde_json::from_value(response)?;
        Ok(member)
    }

    /// Lists guilds of the current user (`/users/@me/guilds`).
    pub async fn list(&self, http: &HttpClient) -> Result<Vec<Guild>> {
        let response = http.get(api_url("/users/@me/guilds")).await?;
        let guilds = serde_json::from_value(response)?;
        Ok(guilds)
    }

    /// Fetches a guild object for the given guild ID. User must be a member of the guild.
    pub async fn get(&self, http: &HttpClient, guild_id: impl AsRef<str>) -> Result<Guild> {
        let response = http
            .get(api_url(&format!("/guilds/{}", guild_id.as_ref())))
            .await?;
        let guild = serde_json::from_value(response)?;
        Ok(guild)
    }

    /// Leaves a guild (`DELETE /users/@me/guilds/{id}`).
    pub async fn leave(&self, http: &HttpClient, guild_id: impl AsRef<str>) -> Result<()> {
        http.delete(api_url(&format!("/users/@me/guilds/{}", guild_id.as_ref())))
            .await?;
        Ok(())
    }

    /// Create a guild (`POST /guilds`). SEE: <https://docs.discord.food/resources/guild#create-guild>
    pub async fn create(&self, http: &HttpClient, data: impl serde::Serialize) -> Result<Guild> {
        let response = http.post(api_url("/guilds"), data).await?;
        let guild = serde_json::from_value(response)?;
        Ok(guild)
    }

    /// Modifies a guild's settings. SEE: <https://docs.discord.food/resources/guild#modify-guild>
    pub async fn edit(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<Guild> {
        let response = http
            .patch(api_url(&format!("/guilds/{}", guild_id.as_ref())), data)
            .await?;
        let guild = serde_json::from_value(response)?;
        Ok(guild)
    }

    /// Modifies the guild's MFA requirement for administrative actions within the guild. User must be the owner.
    pub async fn edit_mfa_level(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        mfa_level: u8,
    ) -> Result<()> {
        http.post(
            api_url(&format!("/guilds/{}/mfa", guild_id.as_ref())),
            json!({ "level": mfa_level }),
        )
        .await?;
        Ok(())
    }

    /// Deletes a guild. User must be the owner.
    pub async fn delete(&self, http: &HttpClient, guild_id: impl AsRef<str>) -> Result<()> {
        http.delete(api_url(&format!("/guilds/{}", guild_id.as_ref())))
            .await?;
        Ok(())
    }

    /// Fetches a list of guild member objects that are members of the guild. SEE: <https://docs.discord.food/resources/guild#get-guild-members>
    pub async fn members(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        limit: Option<u32>,
        after: Option<String>,
    ) -> Result<Vec<Member>> {
        let mut query_params = Vec::new();
        if let Some(limit) = limit {
            query_params.push(format!("limit={limit}"));
        }
        if let Some(after) = after {
            query_params.push(format!("after={after}"));
        }

        let mut url = api_url(&format!("/guilds/{}/members", guild_id.as_ref()));
        if !query_params.is_empty() {
            url.push('?');
            url.push_str(&query_params.join("&"));
        }

        let response = http.get(url).await?;
        let members = serde_json::from_value(response)?;
        Ok(members)
    }

    /// Fetches a list of guild member objects whose username or nickname contains a provided string. User must be a member of the guild.
    pub async fn search_members(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        query: impl AsRef<str>,
        limit: Option<u32>,
    ) -> Result<Vec<Member>> {
        let mut url = api_url(&format!(
            "/guilds/{}/members/search?query={}",
            guild_id.as_ref(),
            query.as_ref()
        ));
        if let Some(limit) = limit {
            url.push_str(&format!("&limit={}", limit));
        }
        let response = http.get(url).await?;
        let members = serde_json::from_value(response)?;
        Ok(members)
    }

    /// Fetches a list of supplemental guild members objects including join source information for the given user IDs. Requires the MANAGE_GUILD permission. SEE: <https://docs.discord.food/resources/guild#get-guild-members-supplemental>
    pub async fn supplemental_members(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        user_ids: Vec<String>,
    ) -> Result<Vec<SupplementalMember>> {
        let response = http
            .post(
                api_url(&format!(
                    "/guilds/{}/members/supplemental",
                    guild_id.as_ref()
                )),
                json!({ "user_ids": user_ids }),
            )
            .await?;
        let members = serde_json::from_value(response)?;
        Ok(members)
    }

    /// Fetches a guild member object for the specified user. (`GET /guilds/{guild.id}/members/{user.id}`).
    pub async fn get_member(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
    ) -> Result<Member> {
        let response = http
            .get(api_url(&format!(
                "/guilds/{}/members/{}",
                guild_id.as_ref(),
                user_id.as_ref()
            )))
            .await?;
        let member = serde_json::from_value(response)?;
        Ok(member)
    }

    /// Modifies attributes of a guild member. (`PATCH /guilds/{guild.id}/members/{user.id}`). SEE: <https://docs.discord.food/resources/guild#modify-guild-member>
    pub async fn edit_member(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<Member> {
        let response = http
            .patch(
                api_url(&format!(
                    "/guilds/{}/members/{}",
                    guild_id.as_ref(),
                    user_id.as_ref()
                )),
                data,
            )
            .await?;
        let member = serde_json::from_value(response)?;
        Ok(member)
    }

    /// Modifies the current user's member in the guild. (`PATCH /guilds/{guild.id}/members/@me`). SEE: <https://docs.discord.food/resources/guild#modify-current-guild-member>
    pub async fn edit_me_member(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<Member> {
        let response = http
            .patch(
                api_url(&format!("/guilds/{}/members/@me", guild_id.as_ref(),)),
                data,
            )
            .await?;
        let member = serde_json::from_value(response)?;
        Ok(member)
    }

    /// Modifies the current user's profile in the guild. (`PATCH /guilds/{guild.id}/members/@me/profile`). SEE: <https://docs.discord.food/resources/guild#modify-guild-member-profile>
    pub async fn edit_me_profile(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<UserProfile> {
        let response = http
            .patch(
                api_url(&format!(
                    "/guilds/{}/members/@me/profile",
                    guild_id.as_ref(),
                )),
                data,
            )
            .await?;
        let profile = serde_json::from_value(response)?;
        Ok(profile)
    }

    /// Adds a role to a guild member. (`PUT /guilds/{guild.id}/members/{user.id}/roles/{role.id}`). SEE: <https://docs.discord.food/resources/guild#add-guild-member-role>
    pub async fn add_member_role(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
        role_id: impl AsRef<str>,
    ) -> Result<()> {
        http.put(
            api_url(&format!(
                "/guilds/{}/members/{}/roles/{}",
                guild_id.as_ref(),
                user_id.as_ref(),
                role_id.as_ref()
            )),
            json!({}),
        )
        .await?;
        Ok(())
    }

    /// Removes a role from a guild member. (`DELETE /guilds/{guild.id}/members/{user.id}/roles/{role.id}`). SEE: <https://docs.discord.food/resources/guild#remove-guild-member-role>
    pub async fn remove_member_role(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
        role_id: impl AsRef<str>,
    ) -> Result<()> {
        http.delete(api_url(&format!(
            "/guilds/{}/members/{}/roles/{}",
            guild_id.as_ref(),
            user_id.as_ref(),
            role_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Removes/kicks a member from a guild (`DELETE /guilds/{guild.id}/members/{user.id}`). SEE: <https://docs.discord.food/resources/guild#remove-guild-member>
    pub async fn kick_member(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
    ) -> Result<()> {
        http.delete(api_url(&format!(
            "/guilds/{}/members/{}",
            guild_id.as_ref(),
            user_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Fetches a list of bans for a guild. (`GET /guilds/{guild.id}/bans`). SEE: <https://docs.discord.food/resources/guild#get-guild-bans>
    pub async fn bans(&self, http: &HttpClient, guild_id: impl AsRef<str>) -> Result<Vec<Ban>> {
        let response = http
            .get(api_url(&format!("/guilds/{}/bans", guild_id.as_ref(),)))
            .await?;
        let bans = serde_json::from_value(response)?;
        Ok(bans)
    }

    /// Fetches a list of ban objects whose username or display name contains a provided string. (`GET /guilds/{guild.id}/bans/search?query={string}`). SEE: <https://docs.discord.food/resources/guild#search-guild-bans>
    pub async fn search_bans(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        query: impl AsRef<str>,
        limit: Option<u8>,
    ) -> Result<Vec<Ban>> {
        let mut url = api_url(&format!(
            "/guilds/{}/bans/search?query={}",
            guild_id.as_ref(),
            query.as_ref()
        ));
        if let Some(limit) = limit {
            url.push_str(&format!("&limit={}", limit));
        }
        let response = http.get(url).await?;
        let bans = serde_json::from_value(response)?;
        Ok(bans)
    }

    /// Fetches a ban object for a user. (`GET /guilds/{guild.id}/bans/{user.id}`). SEE: <https://docs.discord.food/resources/guild#get-guild-ban>
    pub async fn get_ban(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
    ) -> Result<Ban> {
        let response = http
            .get(api_url(&format!(
                "/guilds/{}/bans/{}",
                guild_id.as_ref(),
                user_id.as_ref()
            )))
            .await?;
        let ban = serde_json::from_value(response)?;
        Ok(ban)
    }

    /// Bans a user from the guild. (`PUT /guilds/{guild.id}/bans/{user.id}`). SEE: <https://docs.discord.food/resources/guild#create-guild-ban>
    pub async fn ban_member(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
        delete_message_seconds: Option<u64>,
        reason: Option<&str>,
    ) -> Result<()> {
        let url = api_url(&format!(
            "/guilds/{}/bans/{}",
            guild_id.as_ref(),
            user_id.as_ref()
        ));
        let mut body = json!({});
        if let Some(seconds) = delete_message_seconds {
            body["delete_message_seconds"] = json!(seconds);
        }
        if let Some(reason) = reason {
            body["reason"] = json!(reason);
        }
        http.put(url, body).await?;
        Ok(())
    }

    /// Create multiples bans. (`POST /guilds/{guild.id}/bulk-ban`). SEE: <https://docs.discord.food/resources/guild#bulk-guild-ban>
    pub async fn bulk_ban_members(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<Value> {
        let response = http
            .post(
                api_url(&format!("/guilds/{}/bulk-ban", guild_id.as_ref())),
                data,
            )
            .await?;
        let bans = serde_json::from_value(response)?;
        Ok(bans)
    }

    /// Removes the ban for a user. (`DELETE /guilds/{guild.id}/bans/{user.id}`). SEE: <https://docs.discord.food/resources/guild#delete-guild-ban>
    pub async fn unban_member(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
    ) -> Result<()> {
        http.delete(api_url(&format!(
            "/guilds/{}/bans/{}",
            guild_id.as_ref(),
            user_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Fetches a lit of guild roles (`GET /guilds/{guild.id}/roles`). SEE: <https://docs.discord.food/resources/guild#get-guild-roles>
    pub async fn roles(&self, http: &HttpClient, guild_id: impl AsRef<str>) -> Result<Vec<Role>> {
        let response = http
            .get(api_url(&format!("/guilds/{}/roles", guild_id.as_ref())))
            .await?;
        let roles = serde_json::from_value(response)?;
        Ok(roles)
    }

    /// Fetches a role object for the given role ID. (`GET /guilds/{guild.id}/roles/{role.id}`). SEE: <https://docs.discord.food/resources/guild#get-guild-role>
    pub async fn get_role(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        role_id: impl AsRef<str>,
    ) -> Result<Role> {
        let response = http
            .get(api_url(&format!(
                "/guilds/{}/roles/{}",
                guild_id.as_ref(),
                role_id.as_ref()
            )))
            .await?;
        let role = serde_json::from_value(response)?;
        Ok(role)
    }

    /// Fetches guild role's member counts (`GET /guilds/{guild.id}/roles/{role.id}/members/count`). SEE: <https://docs.discord.food/resources/guild#get-guild-role-members-count>
    /// # Response Example
    /// ```json
    /// {
    /// "1040221495437299782": 2,
    /// "1040221495437299783": 1,
    /// "1040221495437299784": 0
    /// }
    /// ```
    pub async fn get_role_members_count(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        role_id: impl AsRef<str>,
    ) -> Result<Value> {
        let response = http
            .get(api_url(&format!(
                "/guilds/{}/roles/{}/members/count",
                guild_id.as_ref(),
                role_id.as_ref()
            )))
            .await?;
        let counts = serde_json::from_value(response)?;
        Ok(counts)
    }

    /// Fetches a list of member IDs that have a specified role, up to mamximum of 100. (`GET /guilds/{guild.id}/roles/{role.id}/member-ids`). SEE: <https://docs.discord.food/resources/guild#get-guild-role-members>
    /// # Response Example
    /// ```json
    /// ["852892297661906993", "907489667895676928"]
    /// ```
    pub async fn get_role_member_ids(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        role_id: impl AsRef<str>,
    ) -> Result<Vec<String>> {
        let response = http
            .get(api_url(&format!(
                "/guilds/{}/roles/{}/member-ids",
                guild_id.as_ref(),
                role_id.as_ref()
            )))
            .await?;
        let member_ids = serde_json::from_value(response)?;
        Ok(member_ids)
    }

    /// Adds multiple guild members to a role. (`PATCH /guilds/{guild.id}/roles/{role.id}/members`). SEE: <https://docs.discord.food/resources/guild#add-guild-role-members>
    /// # Request Example
    /// ```json
    /// {
    /// "member_ids": ["852892297661906993", "907489667895676928"]
    /// }
    /// ```
    pub async fn add_role_members(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        role_id: impl AsRef<str>,
        member_ids: Vec<String>,
    ) -> Result<Vec<Member>> {
        let response = http
            .patch(
                api_url(&format!(
                    "/guilds/{}/roles/{}/members",
                    guild_id.as_ref(),
                    role_id.as_ref()
                )),
                json!({ "member_ids": member_ids }),
            )
            .await?;
        let members = serde_json::from_value(response)?;
        Ok(members)
    }

    /// Creates a new role for the guild. (`POST /guilds/{guild.id}/roles`). SEE: <https://docs.discord.food/resources/guild#create-guild-role>
    pub async fn create_role(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<Role> {
        let response = http
            .post(
                api_url(&format!("/guilds/{}/roles", guild_id.as_ref())),
                data,
            )
            .await?;
        let role = serde_json::from_value(response)?;
        Ok(role)
    }

    /// Modifiies a role's position. (`PATCH /guilds/{guild.id}/roles`). SEE: <https://docs.discord.food/resources/guild#modify-guild-role-positions>
    pub async fn edit_role_position(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        role_id: impl AsRef<str>,
        position: u32,
    ) -> Result<Vec<Role>> {
        let response = http
            .patch(
                api_url(&format!("/guilds/{}/roles", guild_id.as_ref())),
                json!([{ "id": role_id.as_ref(), "position": position }]),
            )
            .await?;
        let roles = serde_json::from_value(response)?;
        Ok(roles)
    }

    /// Modifies a guild role. (`PATCH /guilds/{guild.id}/roles/{role.id}`). SEE: <https://docs.discord.food/resources/guild#modify-guild-role>
    pub async fn edit_role(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        role_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<Role> {
        let response = http
            .patch(
                api_url(&format!(
                    "/guilds/{}/roles/{}",
                    guild_id.as_ref(),
                    role_id.as_ref()
                )),
                data,
            )
            .await?;
        let role = serde_json::from_value(response)?;
        Ok(role)
    }

    /// Deletes a guild role. (`DELETE /guilds/{guild.id}/roles/{role.id}`). SEE: <https://docs.discord.food/resources/guild#delete-guild-role>
    pub async fn delete_role(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        role_id: impl AsRef<str>,
    ) -> Result<()> {
        http.delete(api_url(&format!(
            "/guilds/{}/roles/{}",
            guild_id.as_ref(),
            role_id.as_ref()
        )))
        .await?;
        Ok(())
    }
}

/// Manager for relationship-related endpoints.
#[derive(Debug, Clone, Copy, Default)]
pub struct RelationshipsManager;

impl RelationshipsManager {
    /// Lists relationships (`GET /users/@me/relationships`).
    pub async fn list(&self, http: &HttpClient) -> Result<Vec<Relationship>> {
        let response = http.get(api_url("/users/@me/relationships")).await?;
        let relationships = serde_json::from_value(response)?;
        Ok(relationships)
    }

    /// Sends a friend request (`POST /users/@me/relationships`) SEE: <https://docs.discord.food/resources/relationships#send-friend-request>
    pub async fn send_friend_request(
        &self,
        http: &HttpClient,
        username: impl AsRef<str>,
    ) -> Result<Relationship> {
        let response = http
            .post(
                api_url("/users/@me/relationships"),
                json!({ "username": username.as_ref() }),
            )
            .await?;
        let relationship = serde_json::from_value::<Relationship>(response)?;
        Ok(relationship)
    }

    /// Blocks a user (`PUT /users/@me/relationships/{id}` with `type=2`).
    pub async fn block(&self, http: &HttpClient, user_id: impl AsRef<str>) -> Result<()> {
        self.put_relationship(http, user_id, 2).await
    }

    /// Removes a relationship (`DELETE /users/@me/relationships/{id}`).
    pub async fn remove(&self, http: &HttpClient, user_id: impl AsRef<str>) -> Result<()> {
        http.delete(api_url(&format!(
            "/users/@me/relationships/{}",
            user_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    async fn put_relationship(
        &self,
        http: &HttpClient,
        user_id: impl AsRef<str>,
        relation_type: u8,
    ) -> Result<()> {
        let response = http
            .put(
                api_url(&format!("/users/@me/relationships/{}", user_id.as_ref())),
                json!({ "type": relation_type }),
            )
            .await?;

        // Discord may return `204 No Content` (mapped to `Null`) or a JSON body.
        let _ = match response {
            Value::Null => None,
            other => Some(other),
        };
        Ok(())
    }

    /// Ignores a user (`PUT /users/@me/relationships/{user.id}/ignore`) SEE: <https://docs.discord.food/resources/relationships#ignore-user>
    pub async fn ignore(&self, http: &HttpClient, user_id: impl AsRef<str>) -> Result<()> {
        http.put(
            api_url(&format!(
                "/users/@me/relationships/{}/ignore",
                user_id.as_ref()
            )),
            json!({}),
        )
        .await?;
        Ok(())
    }

    /// Unignores a user (`DELETE /users/@me/relationships/{user.id}/ignore`) SEE: <https://docs.discord.food/resources/relationships#unignore-user>
    pub async fn unignore(&self, http: &HttpClient, user_id: impl AsRef<str>) -> Result<()> {
        http.delete(api_url(&format!(
            "/users/@me/relationships/{}/ignore",
            user_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Modifies a relationship to another user. (`PATCH /users/@me/relationships/{user.id}`). SEE: <https://docs.discord.food/resources/relationships#modify-relationship>
    pub async fn modify(
        &self,
        http: &HttpClient,
        user_id: impl AsRef<str>,
        nickname: Option<&str>,
    ) -> Result<Relationship> {
        let response = http
            .patch(
                api_url(&format!("/users/@me/relationships/{}", user_id.as_ref())),
                json!({ "nickname": nickname }),
            )
            .await?;
        let relationship = serde_json::from_value(response)?;
        Ok(relationship)
    }

    /// Removes a relationship with another user. (`DELETE /users/@me/relationships/{user.id}`). SEE: <https://docs.discord.food/resources/relationships#remove-relationship>
    pub async fn delete(&self, http: &HttpClient, user_id: impl AsRef<str>) -> Result<()> {
        http.delete(api_url(&format!(
            "/users/@me/relationships/{}",
            user_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Removes multiple relationships. (`POST /users/@me/relationships`). SEE: <https://docs.discord.food/resources/relationships#bulk-remove-relationships>
    /// # Request Example
    /// ```json
    /// {
    /// filters: [1, 2] // 1 for users flagged as SPAM, 2 for IGNORED users
    /// }
    pub async fn bulk_delete(&self, http: &HttpClient, filters: Option<Vec<u8>>) -> Result<()> {
        let url = api_url("/users/@me/relationships");
        let mut body = json!({});
        if let Some(filters) = filters {
            body["filters"] = json!(filters);
        }
        http.post(url, body).await?;
        Ok(())
    }

    /// Searches messages in a guild (`GET /guilds/{guild.id}/messages/search`).
    pub async fn search_messages(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        params: &SearchParams,
    ) -> Result<Value> {
        let url = api_url(&format!(
            "/guilds/{}/messages/search{}",
            guild_id.as_ref(),
            params.to_query_string()
        ));
        let response = http.get(url).await?;
        Ok(response)
    }
}

/// Parameters for message search endpoints.
#[derive(Debug, Clone, Default)]
pub struct SearchParams {
    pub content: Option<String>,
    pub author_id: Option<String>,
    pub channel_id: Option<String>,
    pub has: Option<String>,
    pub min_id: Option<String>,
    pub max_id: Option<String>,
    pub offset: Option<u32>,
}

impl SearchParams {
    fn to_query_string(&self) -> String {
        let mut params = Vec::new();
        if let Some(ref v) = self.content {
            params.push(format!("content={}", urlencoding::encode(v)));
        }
        if let Some(ref v) = self.author_id {
            params.push(format!("author_id={}", v));
        }
        if let Some(ref v) = self.channel_id {
            params.push(format!("channel_id={}", v));
        }
        if let Some(ref v) = self.has {
            params.push(format!("has={}", v));
        }
        if let Some(ref v) = self.min_id {
            params.push(format!("min_id={}", v));
        }
        if let Some(ref v) = self.max_id {
            params.push(format!("max_id={}", v));
        }
        if let Some(v) = self.offset {
            params.push(format!("offset={}", v));
        }
        if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        }
    }
}

/// Manager for channel-related endpoints.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelsManager;

#[derive(Debug, Clone, Default)]
pub struct SearchThreadsParams {
    pub name: Option<String>,
    pub slop: Option<u8>,
    pub tags: Option<Vec<String>>,
    pub tag_setting: Option<String>,
    pub archived: Option<bool>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub limit: Option<u8>,
    pub offset: Option<u32>,
    pub max_id: Option<String>,
    pub min_id: Option<String>,
}

impl ChannelsManager {
    /// Fetches a list of active DM channel objects the user is participating in. (`GET /users/@me/channels`). SEE: <https://docs.discord.food/resources/channel#get-private-channels>
    pub async fn dm_channels(&self, http: &HttpClient) -> Result<Vec<Channel>> {
        let response = http.get(api_url("/users/@me/channels")).await?;
        let channels = serde_json::from_value(response)?;
        Ok(channels)
    }

    /// Fetches an existing DM Channel object with a user. (`GET /users/@me/dms/{user.id}`). SEE: <https://docs.discord.food/resources/channel#get-dm-channel>
    pub async fn get_dm_channel(
        &self,
        http: &HttpClient,
        user_id: impl AsRef<str>,
    ) -> Result<Channel> {
        let response = http
            .get(api_url(&format!("/users/@me/dms/{}", user_id.as_ref())))
            .await?;
        let channel = serde_json::from_value(response)?;
        Ok(channel)
    }

    /// Creates a DM channel with a user or a DM GROUP channel. (`POST /users/@me/channels`). SEE: <https://docs.discord.food/resources/channel#create-private-channel>
    pub async fn create_dm_channel(
        &self,
        http: &HttpClient,
        recipients: Vec<String>,
    ) -> Result<Channel> {
        let response = http
            .post(
                api_url("/users/@me/channels"),
                json!({ "recipients": recipients }),
            )
            .await?;
        let channel = serde_json::from_value(response)?;
        Ok(channel)
    }

    /// Fetches a list of guild channel objects for the guild. (`GET /guilds/{guild.id}/channels`). SEE: <https://docs.discord.food/resources/channel#get-guild-channels>
    pub async fn guild_channels(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
    ) -> Result<Vec<Channel>> {
        let response = http
            .get(api_url(&format!("/guilds/{}/channels", guild_id.as_ref())))
            .await?;
        let channels = serde_json::from_value(response)?;
        Ok(channels)
    }

    /// Creates a new channel in the guild. (`POST /guilds/{guild.id}/channels`). SEE: <https://docs.discord.food/resources/channel#create-guild-channel>
    pub async fn create_guild_channel(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<Channel> {
        let response = http
            .post(
                api_url(&format!("/guilds/{}/channels", guild_id.as_ref())),
                data,
            )
            .await?;
        let channel = serde_json::from_value(response)?;
        Ok(channel)
    }

    /// Modifies the position of a channel. (`PATCH /guilds/{guild.id}/channels`). SEE: <https://docs.discord.food/resources/channel#modify-guild-channel-positions>
    /// # Request Example
    /// ```json
    /// {
    ///     "id": "123456789012345678", // The ID of the channel
    ///     "position": 1 // The new position of the channel
    ///     "lock_permissions": false // Whether to sync the channel's permissions with the new parent category's permissions. Only applicable if the channel is a child of a category and its parent category is being modified.
    ///     "parent_id": "123456789012345678" // The new parent category ID for the channel. Set to null to remove from the current category. Only applicable if the channel is a child of a category and its parent category is being modified.
    /// }
    /// ```
    pub async fn edit_guild_channel_position(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<Vec<Channel>> {
        let response = http
            .patch(
                api_url(&format!("/guilds/{}/channels", guild_id.as_ref())),
                data,
            )
            .await?;
        let channels = serde_json::from_value(response)?;
        Ok(channels)
    }

    /// Fetches a channel object for the given channel ID. User must have access to the channel. (`GET /channels/{channel.id}`). SEE: <https://docs.discord.food/resources/channel#get-channel>
    pub async fn get_channel(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
    ) -> Result<Channel> {
        let response = http
            .get(api_url(&format!("/channels/{}", channel_id.as_ref())))
            .await?;
        let channel = serde_json::from_value(response)?;
        Ok(channel)
    }

    /// Modifies a channel's settings. User must have the MANAGE_CHANNELS permission. (`PATCH /channels/{channel.id}`). SEE: <https://docs.discord.food/resources/channel#modify-channel>
    pub async fn edit_channel(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<Channel> {
        let response = http
            .patch(api_url(&format!("/channels/{}", channel_id.as_ref())), data)
            .await?;
        let channel = serde_json::from_value(response)?;
        Ok(channel)
    }

    /// Deletes a channel or closes a DM channel or leaves a DM GROUP. (`DELETE /channels/{channel.id}`). SEE: <https://docs.discord.food/resources/channel#delete-channel>
    pub async fn delete_channel(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        silent: Option<bool>,
    ) -> Result<()> {
        let mut url = api_url(&format!("/channels/{}", channel_id.as_ref()));
        if let Some(silent) = silent {
            url.push_str(&format!("?silent={}", silent));
        }
        http.delete(url).await?;
        Ok(())
    }

    /// Modifies the permissions for a user or role in a channel. User must have the MANAGE_CHANNELS permission. (`PUT /channels/{channel.id}/permissions/{overwrite.id}`). SEE: <https://docs.discord.food/resources/channel#modify-channel-permissions>
    pub async fn edit_channel_permissions(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        overwrite_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<()> {
        http.put(
            api_url(&format!(
                "/channels/{}/permissions/{}",
                channel_id.as_ref(),
                overwrite_id.as_ref()
            )),
            data,
        )
        .await?;
        Ok(())
    }

    /// Deletes a channel permission overwrite. User must have the MANAGE_CHANNELS permission. (`DELETE /channels/{channel.id}/permissions/{overwrite.id}`). SEE: <https://docs.discord.food/resources/channel#delete-channel-permissions>
    pub async fn delete_channel_permissions(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        overwrite_id: impl AsRef<str>,
    ) -> Result<()> {
        http.delete(api_url(&format!(
            "/channels/{}/permissions/{}",
            channel_id.as_ref(),
            overwrite_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Posts a typing indicator in a channel. (`POST /channels/{channel.id}/typing`). SEE: <https://docs.discord.food/resources/channel#trigger-typing-indicator>
    pub async fn trigger_typing_indicator(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
    ) -> Result<()> {
        http.post(
            api_url(&format!("/channels/{}/typing", channel_id.as_ref())),
            json!({}),
        )
        .await?;
        Ok(())
    }

    /// Checks if the current user is eligible to ring a call in the DM channel. (`GET /channels/{channel.id}/call`). SEE: <https://docs.discord.food/resources/channel#get-call-eligibility>
    pub async fn check_call_eligibility(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
    ) -> Result<bool> {
        let response = http
            .get(api_url(&format!("/channels/{}/call", channel_id.as_ref())))
            .await?;
        let data: serde_json::Value = serde_json::from_value(response)?;
        Ok(data["ringable"].as_bool().unwrap_or(false))
    }

    /// Modifies the active call in the private channel. (`PATCH /channels/{channel.id}/call`). SEE: <https://docs.discord.food/resources/channel#modify-call>
    /// # Request Example
    /// ```json
    /// {
    /// "region": "us-west", // The RTC region to connect to for this call
    /// }
    /// ```
    pub async fn modify_call(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<()> {
        http.patch(
            api_url(&format!("/channels/{}/call", channel_id.as_ref())),
            data,
        )
        .await?;
        Ok(())
    }

    /// Rings the recipients of a private channel to notify them of an active call. (`POST /channels/{channel.id}/call/ring`). SEE: <https://docs.discord.food/resources/channel#ring-channel-recipients>
    pub async fn ring_call_recipients(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        recipients: Vec<String>,
    ) -> Result<()> {
        http.post(
            api_url(&format!("/channels/{}/call/ring", channel_id.as_ref())),
            json!({ "recipients": recipients }),
        )
        .await?;
        Ok(())
    }

    /// Stops ringing the recipients of a private channel. (`POST /channels/{channel.id}/call/stop-ringing`). SEE: <https://docs.discord.food/resources/channel#stop-ringing-channel-recipients>
    pub async fn stop_ringing_call_recipients(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        recipients: Vec<String>,
    ) -> Result<()> {
        http.post(
            api_url(&format!(
                "/channels/{}/call/stop-ringing",
                channel_id.as_ref()
            )),
            json!({ "recipients": recipients }),
        )
        .await?;
        Ok(())
    }

    /// Adds a recipient to a private channel. (`PUT /channels/{channel.id}/recipients/{user.id}`). SEE: <https://docs.discord.food/resources/channel#add-channel-recipient>
    /// # More Info
    /// - If operation on a DM, returns a Group DM channel object on success.
    pub async fn add_recipient(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
    ) -> Result<Option<Channel>> {
        let response = http
            .put(
                api_url(&format!(
                    "/channels/{}/recipients/{}",
                    channel_id.as_ref(),
                    user_id.as_ref()
                )),
                json!({}),
            )
            .await?;

        match response {
            serde_json::Value::Null => Ok(None),
            value => Ok(Some(serde_json::from_value::<Channel>(value)?)),
        }
    }

    /// Removes a recipient from a private channel. (`DELETE /channels/{channel.id}/recipients/{user.id}`). SEE: <https://docs.discord.food/resources/channel#remove-channel-recipient>
    /// # More Info
    /// - You have to be the owner of the Group DM.
    pub async fn remove_recipient(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
    ) -> Result<()> {
        http.delete(api_url(&format!(
            "/channels/{}/recipients/{}",
            channel_id.as_ref(),
            user_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Modifies a message request's status. (`PUT /channels/{channel.id}/recipients/@me`) SEE: <https://docs.discord.food/resources/channel#update-message-request>
    /// # consent_status
    /// - 0: UNSPECIFIED - The DM isn't a message request
    /// - 1: PENDING - The message request is pending
    /// - 2: ACCEPTED - The message request has been accepted
    /// - 3: REJECTED - The message request has been rejected
    pub async fn update_message_request(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        consent_status: u8,
    ) -> Result<Channel> {
        let response = http
            .put(
                api_url(&format!("/channels/{}/recipients/@me", channel_id.as_ref())),
                json!({ "consent_status": consent_status }),
            )
            .await?;

        let channel = serde_json::from_value(response)?;
        Ok(channel)
    }

    /// Rejects and deletes a pending message request (`DELETE /channels/{channel.id}/recipients/@me`). SEE: <https://docs.discord.food/resources/channel#delete-message-request>
    pub async fn delete_message_request(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
    ) -> Result<()> {
        http.delete(api_url(&format!(
            "/channels/{}/recipients/@me",
            channel_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Rejects and deletes multiple pending message requests (`PUT /channels/recipients/@me/batch-reject`). SEE: <https://docs.discord.food/resources/channel#batch-reject-message-requests>
    pub async fn batch_reject_message_requests(
        &self,
        http: &HttpClient,
        channel_ids: Vec<String>,
    ) -> Result<()> {
        http.put(
            api_url("/channels/recipients/@me/batch-reject"),
            json!({ "channel_ids": channel_ids }),
        )
        .await?;
        Ok(())
    }

    /// Returns a list of supplemetal message request objects with the message that triggered each message request (`GET /users/@me/message-requests/supplemental-data`). SEE: <https://docs.discord.food/resources/channel#get-supplemental-message-request>
    pub async fn get_supplemental_message_request_data(
        &self,
        http: &HttpClient,
    ) -> Result<Vec<SupplementalMessageRequest>> {
        let response = http
            .get(api_url("/users/@me/message-requests/supplemental-data"))
            .await?;
        let data = serde_json::from_value(response)?;
        Ok(data)
    }

    /// Returns all active threads in the guild (`GET /guilds/{guild.id}/threads/active`). SEE: <https://docs.discord.food/resources/channel#get-guild-active-threads>
    /// # Response Example
    /// ```json
    /// {
    /// "threads": Vec<Channel>, // The list of active threads in the guild
    /// "members": Vec<ThreadMember> // The list of thread member objects for the active threads
    /// }
    pub async fn active_threads(
        &self,
        http: &HttpClient,
        guild_id: impl AsRef<str>,
    ) -> Result<Value> {
        let response = http
            .get(api_url(&format!(
                "/guilds/{}/threads/active",
                guild_id.as_ref()
            )))
            .await?;
        let threads = serde_json::from_value(response)?;
        Ok(threads)
    }

    /// Returns archived threads in the channel that are public. (`GET /channels/{channel.id}/threads/archived/public`). SEE: <https://docs.discord.food/resources/channel#get-public-archived-threads>
    /// # More Info
    /// - before?: ISO8601 timestamp to get threads before a certain time
    /// - limit?: Maximum number of threads to return (2-100, default 50)
    /// # Response Example
    /// ```json
    /// {
    /// "threads": Vec<Channel>, // The list of archived threads in the channel
    /// "members": Vec<ThreadMember> // The list of thread member objects for the archived threads
    /// "has_more": bool // Whether there are more threads that can be returned with a subsequent request
    /// }
    /// ```
    pub async fn public_archived_threads(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        before: Option<&str>,
        limit: Option<u8>,
    ) -> Result<Value> {
        let mut url = api_url(&format!(
            "/channels/{}/threads/archived/public",
            channel_id.as_ref()
        ));
        if let Some(before) = before {
            url.push_str(&format!("?before={}", before));
        }
        if let Some(limit) = limit {
            url.push_str(&format!("&limit={}", limit));
        }
        let response = http.get(url).await?;
        let threads = serde_json::from_value(response)?;
        Ok(threads)
    }

    /// Returns archived threads in the channel that are private. (`GET /channels/{channel.id}/threads/archived/private`). SEE: <https://docs.discord.food/resources/channel#get-private-archived-threads>
    /// # More Info
    /// - before?: ISO8601 timestamp to get threads before a certain time
    /// - limit?: Maximum number of threads to return (2-100, default 50)
    /// # Response Example
    /// ```json
    /// {
    /// "threads": Vec<Channel>, // The list of archived threads in the channel
    /// "members": Vec<ThreadMember> // The list of thread member objects for the archived threads
    /// "has_more": bool // Whether there are more threads that can be returned with a subsequent request
    /// }
    pub async fn private_archived_threads(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        before: Option<&str>,
        limit: Option<u8>,
    ) -> Result<Value> {
        let mut url = api_url(&format!(
            "/channels/{}/threads/archived/private",
            channel_id.as_ref()
        ));
        if let Some(before) = before {
            url.push_str(&format!("?before={}", before));
        }
        if let Some(limit) = limit {
            url.push_str(&format!("&limit={}", limit));
        }
        let response = http.get(url).await?;
        let threads = serde_json::from_value(response)?;
        Ok(threads)
    }

    /// Returns archived threads in the channel that the user has joined. (`GET /channels/{channel.id}/users/@me/threads/archived/private`). SEE: <https://docs.discord.food/resources/channel#get-joined-private-archived-threads>
    /// # More Info
    /// - before?: ISO8601 timestamp to get threads before a certain time
    /// - limit?: Maximum number of threads to return (2-100, default 50)
    /// # Response Example
    /// ```json
    /// {
    /// "threads": Vec<Channel>, // The list of archived threads in the channel
    /// "members": Vec<ThreadMember> // The list of thread member objects for the archived threads
    /// "has_more": bool // Whether there are more threads that can be returned with a subsequent request
    /// }
    /// ```
    pub async fn joined_private_archived_threads(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        before: Option<&str>,
        limit: Option<u8>,
    ) -> Result<Value> {
        let mut url = api_url(&format!(
            "/channels/{}/users/@me/threads/archived/private",
            channel_id.as_ref()
        ));
        if let Some(before) = before {
            url.push_str(&format!("?before={}", before));
        }
        if let Some(limit) = limit {
            url.push_str(&format!("&limit={}", limit));
        }
        let response = http.get(url).await?;
        let threads = serde_json::from_value(response)?;
        Ok(threads)
    }

    /// Returns threads in the channel that match the search parameters. (`GET /channels/{channel.id}/threads/search`). SEE: <https://docs.discord.food/resources/channel#search-threads>
    pub async fn search_threads(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        params: SearchThreadsParams,
    ) -> Result<Value> {
        let mut url = api_url(&format!("/channels/{}/threads/search", channel_id.as_ref()));
        let mut query_params = Vec::new();
        if let Some(name) = params.name {
            query_params.push(format!("name={}", name));
        }
        if let Some(slop) = params.slop {
            query_params.push(format!("slop={}", slop));
        }
        if let Some(tag) = params.tags {
            for t in tag {
                query_params.push(format!("tag={}", t));
            }
        }
        if let Some(tag_setting) = params.tag_setting {
            query_params.push(format!("tag_setting={}", tag_setting));
        }
        if let Some(archived) = params.archived {
            query_params.push(format!("archived={}", archived));
        }
        if let Some(sort_by) = params.sort_by {
            query_params.push(format!("sort_by={}", sort_by));
        }
        if let Some(sort_order) = params.sort_order {
            query_params.push(format!("sort_order={}", sort_order));
        }
        if let Some(limit) = params.limit {
            query_params.push(format!("limit={}", limit));
        }
        if let Some(offset) = params.offset {
            query_params.push(format!("offset={}", offset));
        }
        if let Some(max_id) = params.max_id {
            query_params.push(format!("max_id={}", max_id));
        }
        if let Some(min_id) = params.min_id {
            query_params.push(format!("min_id={}", min_id));
        }
        if !query_params.is_empty() {
            url.push_str(&format!("?{}", query_params.join("&")));
        }

        let response = http.get(url).await?;
        let threads = serde_json::from_value(response)?;
        Ok(threads)
    }

    /// Creates a new thread from an existing message. (`POST /channels/{channel.id}/messages/{message.id}/threads`). SEE: <https://docs.discord.food/resources/channel#create-thread-from-message>
    /// # Request Example
    /// ```json
    /// {
    /// "name": "thread name", // The name of the thread (1-100 characters)
    /// "auto_archive_duration": 60, // The duration in minutes to automatically archive the thread (60, 1440, 4320, 10080, default 4320)
    /// "rate_limit_per_user": 120 // The rate limit per user in seconds (0-21600)
    /// }
    /// ```
    pub async fn create_thread_from_message(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        message_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<Channel> {
        let response = http
            .post(
                api_url(&format!(
                    "/channels/{}/messages/{}/threads",
                    channel_id.as_ref(),
                    message_id.as_ref()
                )),
                data,
            )
            .await?;
        let thread = serde_json::from_value(response)?;
        Ok(thread)
    }

    /// Creates a new thread without an existing message. (`POST /channels/{channel.id}/threads`). SEE: <https://docs.discord.food/resources/channel#create-thread>
    /// # Request Example
    /// ```json
    /// {
    /// "name": "thread name", // The name of the thread (1-100 characters)
    /// "auto_archive_duration": 60, // The duration in minutes to automatically archive the thread (60, 1440, 4320, 10080, default 4320)
    /// "rate_limit_per_user": 120 // The rate limit per user in seconds (0-21600)
    /// "type"?: 11 // The type of thread to create (10 for public thread, 11 for private thread, default 11)
    /// "applied_tags"?: ["tag_id"] // The IDs of the applied tags (0-5)
    /// "message"?: thread-only message object
    /// }
    /// ```
    pub async fn create_thread(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<Channel> {
        let response = http
            .post(
                api_url(&format!("/channels/{}/threads", channel_id.as_ref())),
                data,
            )
            .await?;
        let thread = serde_json::from_value(response)?;
        Ok(thread)
    }

    /// Adds the current user to a thread. (`PUT /channels/{channel.id}/thread-members/@me`). SEE: <https://docs.discord.food/resources/channel#join-thread>
    pub async fn join_thread(&self, http: &HttpClient, channel_id: impl AsRef<str>) -> Result<()> {
        http.put(
            api_url(&format!(
                "/channels/{}/thread-members/@me",
                channel_id.as_ref()
            )),
            json!({}),
        )
        .await?;
        Ok(())
    }

    /// Adds another member to a thread. (`PUT /channels/{channel.id}/thread-members/{user.id}`). SEE: <https://docs.discord.food/resources/channel#add-thread-member>
    pub async fn add_thread_member(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
    ) -> Result<()> {
        http.put(
            api_url(&format!(
                "/channels/{}/thread-members/{}",
                channel_id.as_ref(),
                user_id.as_ref()
            )),
            json!({}),
        )
        .await?;
        Ok(())
    }

    /// Updates the current user's thread settings. User must be a member of the thread. (`PATCH /channels/{channel.id}/thread-members/@me/settings`). SEE: <https://docs.discord.food/resources/channel#modify-thread-settings>
    /// # Request Example
    /// ```json
    /// {
    /// "flags"?: u32 // The thread member's flags to update. Currently only supports the following flag: 1 for NOTIFY_ON_NEW_MESSAGES
    /// "muted"?: bool // Whether the user had muted the thread.
    /// "mute_config"?: mute config object // The muted metadata for the thread.
    /// }
    /// ```
    pub async fn edit_thread_me_settings(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        data: impl serde::Serialize,
    ) -> Result<()> {
        http.patch(
            api_url(&format!(
                "/channels/{}/thread-members/@me/settings",
                channel_id.as_ref()
            )),
            data,
        )
        .await?;
        Ok(())
    }

    /// Removes the current user from a thread. (`DELETE /channels/{channel.id}/thread-members/@me`). SEE: <https://docs.discord.food/resources/channel#leave-thread>
    pub async fn leave_thread(&self, http: &HttpClient, channel_id: impl AsRef<str>) -> Result<()> {
        http.delete(api_url(&format!(
            "/channels/{}/thread-members/@me",
            channel_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Removes another member from a thread. (`DELETE /channels/{channel.id}/thread-members/{user.id}`). SEE: <https://docs.discord.food/resources/channel#remove-thread-member>
    pub async fn remove_thread_member(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
    ) -> Result<()> {
        http.delete(api_url(&format!(
            "/channels/{}/thread-members/{}",
            channel_id.as_ref(),
            user_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Creates a new tag in the thread-only channel. (`POST /channels/{channel.id}/tags`). SEE: <https://docs.discord.food/resources/channel#create-channel-tag>
    pub async fn create_channel_tag(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        data: ForumTag,
    ) -> Result<Channel> {
        let response = http
            .post(
                api_url(&format!("/channels/{}/tags", channel_id.as_ref())),
                data,
            )
            .await?;
        let channel = serde_json::from_value(response)?;
        Ok(channel)
    }

    /// Replaces a tag in the thread-only channel. (`PUT /channels/{channel.id}/tags/{tag.id}`). SEE: <https://docs.discord.food/resources/channel#modify-channel-tag>
    pub async fn edit_channel_tag(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        tag_id: impl AsRef<str>,
        data: ForumTag,
    ) -> Result<Channel> {
        let response = http
            .put(
                api_url(&format!(
                    "/channels/{}/tags/{}",
                    channel_id.as_ref(),
                    tag_id.as_ref()
                )),
                data,
            )
            .await?;
        let channel = serde_json::from_value(response)?;
        Ok(channel)
    }

    /// Deletes a tag in the thread-only channel. (`DELETE /channels/{channel.id}/tags/{tag.id}`). SEE: <https://docs.discord.food/resources/channel#delete-channel-tag>
    pub async fn delete_channel_tag(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        tag_id: impl AsRef<str>,
    ) -> Result<()> {
        http.delete(api_url(&format!(
            "/channels/{}/tags/{}",
            channel_id.as_ref(),
            tag_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    // ==================== Message Endpoints ====================

    /// Sends a message to a channel using a `CreateMessage` builder.
    pub async fn send_message(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        message: CreateMessage,
    ) -> Result<Message> {
        let url = api_url(&format!("/channels/{}/messages", channel_id.as_ref()));
        let response = http.post(url, message).await?;
        let msg = serde_json::from_value(response)?;
        Ok(msg)
    }

    /// Fetches message history for a channel (`GET /channels/{channel.id}/messages`).
    pub async fn get_messages(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        limit: Option<u32>,
        before: Option<String>,
        after: Option<String>,
        around: Option<String>,
    ) -> Result<Vec<Message>> {
        let mut params = Vec::new();
        if let Some(l) = limit {
            params.push(format!("limit={}", l.min(100)));
        }
        if let Some(ref b) = before {
            params.push(format!("before={}", b));
        }
        if let Some(ref a) = after {
            params.push(format!("after={}", a));
        }
        if let Some(ref a) = around {
            params.push(format!("around={}", a));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        let url = api_url(&format!(
            "/channels/{}/messages{}",
            channel_id.as_ref(),
            query
        ));
        let response = http.get(url).await?;
        let messages = serde_json::from_value(response)?;
        Ok(messages)
    }

    /// Searches messages in a channel (`GET /channels/{channel.id}/messages/search`).
    pub async fn search_messages(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        params: &SearchParams,
    ) -> Result<Value> {
        let url = api_url(&format!(
            "/channels/{}/messages/search{}",
            channel_id.as_ref(),
            params.to_query_string()
        ));
        let response = http.get(url).await?;
        Ok(response)
    }

    // ==================== Pin Endpoints ====================

    /// Pins a message in a channel (`PUT /channels/{channel.id}/pins/{message.id}`).
    pub async fn pin_message(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        message_id: impl AsRef<str>,
    ) -> Result<()> {
        http.put(
            api_url(&format!(
                "/channels/{}/pins/{}",
                channel_id.as_ref(),
                message_id.as_ref()
            )),
            json!({}),
        )
        .await?;
        Ok(())
    }

    /// Unpins a message in a channel (`DELETE /channels/{channel.id}/pins/{message.id}`).
    pub async fn unpin_message(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
        message_id: impl AsRef<str>,
    ) -> Result<()> {
        http.delete(api_url(&format!(
            "/channels/{}/pins/{}",
            channel_id.as_ref(),
            message_id.as_ref()
        )))
        .await?;
        Ok(())
    }

    /// Gets all pinned messages in a channel (`GET /channels/{channel.id}/pins`).
    pub async fn get_pinned_messages(
        &self,
        http: &HttpClient,
        channel_id: impl AsRef<str>,
    ) -> Result<Vec<Message>> {
        let url = api_url(&format!("/channels/{}/pins", channel_id.as_ref()));
        let response = http.get(url).await?;
        let messages = serde_json::from_value(response)?;
        Ok(messages)
    }
}
