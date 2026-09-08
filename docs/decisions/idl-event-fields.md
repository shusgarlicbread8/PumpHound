# Pinned IDL event and account fields
Generated from vendored IDLs for decoder implementation.

# pump.json address=6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P
## event AdminSetCreatorEvent disc=[64, 69, 192, 104, 29, 30, 25, 107]
- `timestamp`: `i64`
- `admin_set_creator_authority`: `pubkey`
- `mint`: `pubkey`
- `bonding_curve`: `pubkey`
- `old_creator`: `pubkey`
- `new_creator`: `pubkey`
## event AdminSetIdlAuthorityEvent disc=[245, 59, 70, 34, 75, 185, 109, 92]
- `idl_authority`: `pubkey`
## event AdminUpdateTokenIncentivesEvent disc=[147, 250, 108, 120, 247, 29, 67, 222]
- `start_time`: `i64`
- `end_time`: `i64`
- `day_number`: `u64`
- `token_supply_per_day`: `u64`
- `mint`: `pubkey`
- `seconds_in_a_day`: `i64`
- `timestamp`: `i64`
## event ClaimCashbackEvent disc=[226, 214, 246, 33, 7, 242, 147, 229]
- `user`: `pubkey`
- `amount`: `u64`
- `timestamp`: `i64`
- `total_claimed`: `u64`
- `total_cashback_earned`: `u64`
## event ClaimTokenIncentivesEvent disc=[79, 172, 246, 49, 205, 91, 206, 232]
- `user`: `pubkey`
- `mint`: `pubkey`
- `amount`: `u64`
- `timestamp`: `i64`
- `total_claimed_tokens`: `u64`
- `current_sol_volume`: `u64`
## event CloseUserVolumeAccumulatorEvent disc=[146, 159, 189, 172, 146, 88, 56, 244]
- `user`: `pubkey`
- `timestamp`: `i64`
- `total_unclaimed_tokens`: `u64`
- `total_claimed_tokens`: `u64`
- `current_sol_volume`: `u64`
- `last_update_timestamp`: `i64`
## event CollectCreatorFeeEvent disc=[122, 2, 127, 1, 14, 191, 12, 175]
- `timestamp`: `i64`
- `creator`: `pubkey`
- `creator_fee`: `u64`
- `quote_mint`: `pubkey`
## event CompleteEvent disc=[95, 114, 97, 156, 212, 46, 152, 8]
- `user`: `pubkey`
- `mint`: `pubkey`
- `bonding_curve`: `pubkey`
- `timestamp`: `i64`
- `quote_mint`: `pubkey`
## event CompletePumpAmmMigrationEvent disc=[189, 233, 93, 185, 92, 148, 234, 148]
- `user`: `pubkey`
- `mint`: `pubkey`
- `mint_amount`: `u64`
- `sol_amount`: `u64`
- `pool_migration_fee`: `u64`
- `bonding_curve`: `pubkey`
- `timestamp`: `i64`
- `pool`: `pubkey`
- `quote_mint`: `pubkey`
## event CreateEvent disc=[27, 114, 169, 77, 222, 235, 99, 118]
- `name`: `string`
- `symbol`: `string`
- `uri`: `string`
- `mint`: `pubkey`
- `bonding_curve`: `pubkey`
- `user`: `pubkey`
- `creator`: `pubkey`
- `timestamp`: `i64`
- `virtual_token_reserves`: `u64`
- `virtual_sol_reserves`: `u64`
- `real_token_reserves`: `u64`
- `token_total_supply`: `u64`
- `token_program`: `pubkey`
- `is_mayhem_mode`: `bool`
- `is_cashback_enabled`: `bool`
- `quote_mint`: `pubkey`
- `virtual_quote_reserves`: `u64`
## event DistributeCreatorFeesEvent disc=[165, 55, 129, 112, 4, 179, 202, 40]
- `timestamp`: `i64`
- `mint`: `pubkey`
- `bonding_curve`: `pubkey`
- `sharing_config`: `pubkey`
- `admin`: `pubkey`
- `shareholders`: `{'vec': {'defined': {'name': 'Shareholder'}}}`
- `distributed`: `u64`
- `quote_mint`: `pubkey`
## event ExtendAccountEvent disc=[97, 97, 215, 144, 93, 146, 22, 124]
- `account`: `pubkey`
- `user`: `pubkey`
- `current_size`: `u64`
- `new_size`: `u64`
- `timestamp`: `i64`
## event InitUserVolumeAccumulatorEvent disc=[134, 36, 13, 72, 232, 101, 130, 216]
- `payer`: `pubkey`
- `user`: `pubkey`
- `timestamp`: `i64`
## event MigrateBondingCurveCreatorEvent disc=[155, 167, 104, 220, 213, 108, 243, 3]
- `timestamp`: `i64`
- `mint`: `pubkey`
- `bonding_curve`: `pubkey`
- `sharing_config`: `pubkey`
- `old_creator`: `pubkey`
- `new_creator`: `pubkey`
## event MinimumDistributableFeeEvent disc=[168, 216, 132, 239, 235, 182, 49, 52]
- `minimum_required`: `u64`
- `distributable_fees`: `u64`
- `can_distribute`: `bool`
## event ReservedFeeRecipientsEvent disc=[43, 188, 250, 18, 221, 75, 187, 95]
- `timestamp`: `i64`
- `reserved_fee_recipient`: `pubkey`
- `reserved_fee_recipients`: `{'array': ['pubkey', 7]}`
## event SetCreatorEvent disc=[237, 52, 123, 37, 245, 251, 72, 210]
- `timestamp`: `i64`
- `mint`: `pubkey`
- `bonding_curve`: `pubkey`
- `creator`: `pubkey`
## event SetMetaplexCreatorEvent disc=[142, 203, 6, 32, 127, 105, 191, 162]
- `timestamp`: `i64`
- `mint`: `pubkey`
- `bonding_curve`: `pubkey`
- `metadata`: `pubkey`
- `creator`: `pubkey`
## event SetParamsEvent disc=[223, 195, 159, 246, 62, 48, 143, 131]
- `initial_virtual_token_reserves`: `u64`
- `initial_virtual_sol_reserves`: `u64`
- `initial_real_token_reserves`: `u64`
- `final_real_sol_reserves`: `u64`
- `token_total_supply`: `u64`
- `fee_basis_points`: `u64`
- `withdraw_authority`: `pubkey`
- `enable_migrate`: `bool`
- `pool_migration_fee`: `u64`
- `creator_fee_basis_points`: `u64`
- `fee_recipients`: `{'array': ['pubkey', 8]}`
- `timestamp`: `i64`
- `set_creator_authority`: `pubkey`
- `admin_set_creator_authority`: `pubkey`
## event SyncUserVolumeAccumulatorEvent disc=[197, 122, 167, 124, 116, 81, 91, 255]
- `user`: `pubkey`
- `total_claimed_tokens_before`: `u64`
- `total_claimed_tokens_after`: `u64`
- `timestamp`: `i64`
## event TradeEvent disc=[189, 219, 127, 211, 78, 230, 97, 238]
- `mint`: `pubkey`
- `sol_amount`: `u64`
- `token_amount`: `u64`
- `is_buy`: `bool`
- `user`: `pubkey`
- `timestamp`: `i64`
- `virtual_sol_reserves`: `u64`
- `virtual_token_reserves`: `u64`
- `real_sol_reserves`: `u64`
- `real_token_reserves`: `u64`
- `fee_recipient`: `pubkey`
- `fee_basis_points`: `u64`
- `fee`: `u64`
- `creator`: `pubkey`
- `creator_fee_basis_points`: `u64`
- `creator_fee`: `u64`
- `track_volume`: `bool`
- `total_unclaimed_tokens`: `u64`
- `total_claimed_tokens`: `u64`
- `current_sol_volume`: `u64`
- `last_update_timestamp`: `i64`
- `ix_name`: `string`
- `mayhem_mode`: `bool`
- `cashback_fee_basis_points`: `u64`
- `cashback`: `u64`
- `buyback_fee_basis_points`: `u64`
- `buyback_fee`: `u64`
- `shareholders`: `{'vec': {'defined': {'name': 'Shareholder'}}}`
- `quote_mint`: `pubkey`
- `quote_amount`: `u64`
- `virtual_quote_reserves`: `u64`
- `real_quote_reserves`: `u64`
## event UpdateGlobalAuthorityEvent disc=[182, 195, 137, 42, 35, 206, 207, 247]
- `global`: `pubkey`
- `authority`: `pubkey`
- `new_authority`: `pubkey`
- `timestamp`: `i64`
## event UpdateMayhemVirtualParamsEvent disc=[117, 123, 228, 182, 161, 168, 220, 214]
- `timestamp`: `i64`
- `mint`: `pubkey`
- `virtual_token_reserves`: `u64`
- `virtual_sol_reserves`: `u64`
- `new_virtual_token_reserves`: `u64`
- `new_virtual_sol_reserves`: `u64`
- `real_token_reserves`: `u64`
- `real_sol_reserves`: `u64`
## account BondingCurve disc=[23, 183, 248, 55, 96, 216, 172, 96]
- `virtual_token_reserves`: `u64`
- `virtual_quote_reserves`: `u64`
- `real_token_reserves`: `u64`
- `real_quote_reserves`: `u64`
- `token_total_supply`: `u64`
- `complete`: `bool`
- `creator`: `pubkey`
- `is_mayhem_mode`: `bool`
- `is_cashback_coin`: `bool`
- `quote_mint`: `pubkey`
## account FeeConfig disc=[143, 52, 146, 187, 219, 123, 76, 155]
- `bump`: `u8`
- `admin`: `pubkey`
- `flat_fees`: `{'defined': {'name': 'Fees'}}`
- `fee_tiers`: `{'vec': {'defined': {'name': 'FeeTier'}}}`
- `stable_fee_tiers`: `{'vec': {'defined': {'name': 'FeeTier'}}}`
## account Global disc=[167, 232, 232, 177, 200, 108, 114, 127]
- `initialized`: `bool`
- `authority`: `pubkey`
- `fee_recipient`: `pubkey`
- `initial_virtual_token_reserves`: `u64`
- `initial_virtual_sol_reserves`: `u64`
- `initial_real_token_reserves`: `u64`
- `token_total_supply`: `u64`
- `fee_basis_points`: `u64`
- `withdraw_authority`: `pubkey`
- `enable_migrate`: `bool`
- `pool_migration_fee`: `u64`
- `creator_fee_basis_points`: `u64`
- `fee_recipients`: `{'array': ['pubkey', 7]}`
- `set_creator_authority`: `pubkey`
- `admin_set_creator_authority`: `pubkey`
- `create_v2_enabled`: `bool`
- `whitelist_pda`: `pubkey`
- `reserved_fee_recipient`: `pubkey`
- `mayhem_mode_enabled`: `bool`
- `reserved_fee_recipients`: `{'array': ['pubkey', 7]}`
- `is_cashback_enabled`: `bool`
- `buyback_fee_recipients`: `{'array': ['pubkey', 8]}`
- `buyback_basis_points`: `u64`
- `initial_virtual_quote_reserves`: `u64`
- `whitelisted_quote_mints`: `{'array': ['pubkey', 1]}`
## account GlobalVolumeAccumulator disc=[202, 42, 246, 43, 142, 190, 30, 255]
- `start_time`: `i64`
- `end_time`: `i64`
- `seconds_in_a_day`: `i64`
- `mint`: `pubkey`
- `total_token_supply`: `{'array': ['u64', 30]}`
- `sol_volumes`: `{'array': ['u64', 30]}`
## account SharingConfig disc=[216, 74, 9, 0, 56, 140, 93, 75]
- `bump`: `u8`
- `version`: `u8`
- `status`: `{'defined': {'name': 'ConfigStatus'}}`
- `mint`: `pubkey`
- `admin`: `pubkey`
- `admin_revoked`: `bool`
- `shareholders`: `{'vec': {'defined': {'name': 'Shareholder'}}}`
## account UserVolumeAccumulator disc=[86, 255, 112, 14, 102, 53, 154, 250]
- `user`: `pubkey`
- `needs_claim`: `bool`
- `total_unclaimed_tokens`: `u64`
- `total_claimed_tokens`: `u64`
- `current_sol_volume`: `u64`
- `last_update_timestamp`: `i64`
- `has_total_claimed_tokens`: `bool`
- `cashback_earned`: `u64`
- `total_cashback_claimed`: `u64`
- `stable_cashback_earned`: `u64`
- `total_stable_cashback_claimed`: `u64`

# pump_amm.json address=pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA
## event AdminSetCoinCreatorEvent disc=[45, 220, 93, 24, 25, 97, 172, 104]
- `timestamp`: `i64`
- `admin_set_coin_creator_authority`: `pubkey`
- `base_mint`: `pubkey`
- `pool`: `pubkey`
- `old_coin_creator`: `pubkey`
- `new_coin_creator`: `pubkey`
## event AdminUpdateTokenIncentivesEvent disc=[147, 250, 108, 120, 247, 29, 67, 222]
- `start_time`: `i64`
- `end_time`: `i64`
- `day_number`: `u64`
- `token_supply_per_day`: `u64`
- `mint`: `pubkey`
- `seconds_in_a_day`: `i64`
- `timestamp`: `i64`
## event BoostBuyAndBurnEvent disc=[63, 69, 28, 22, 48, 92, 194, 185]
- `timestamp`: `i64`
- `mint`: `pubkey`
- `bonding_curve`: `pubkey`
- `pool`: `pubkey`
- `authority`: `pubkey`
- `quote_amount_in_requested`: `u64`
- `quote_amount_in_used`: `u64`
- `base_amount_burned`: `u64`
- `virtual_quote_reserves`: `i128`
- `real_quote_reserves_after`: `u64`
- `base_reserves_after`: `u64`
- `boost_vault_remaining`: `u64`
## event BuyEvent disc=[103, 244, 82, 31, 44, 245, 119, 119]
- `timestamp`: `i64`
- `base_amount_out`: `u64`
- `max_quote_amount_in`: `u64`
- `user_base_token_reserves`: `u64`
- `user_quote_token_reserves`: `u64`
- `pool_base_token_reserves`: `u64`
- `pool_quote_token_reserves`: `u64`
- `quote_amount_in`: `u64`
- `lp_fee_basis_points`: `u64`
- `lp_fee`: `u64`
- `protocol_fee_basis_points`: `u64`
- `protocol_fee`: `u64`
- `quote_amount_in_with_lp_fee`: `u64`
- `user_quote_amount_in`: `u64`
- `pool`: `pubkey`
- `user`: `pubkey`
- `user_base_token_account`: `pubkey`
- `user_quote_token_account`: `pubkey`
- `protocol_fee_recipient`: `pubkey`
- `protocol_fee_recipient_token_account`: `pubkey`
- `coin_creator`: `pubkey`
- `coin_creator_fee_basis_points`: `u64`
- `coin_creator_fee`: `u64`
- `track_volume`: `bool`
- `total_unclaimed_tokens`: `u64`
- `total_claimed_tokens`: `u64`
- `current_sol_volume`: `u64`
- `last_update_timestamp`: `i64`
- `min_base_amount_out`: `u64`
- `ix_name`: `string`
- `cashback_fee_basis_points`: `u64`
- `cashback`: `u64`
- `buyback_fee_basis_points`: `u64`
- `buyback_fee`: `u64`
- `virtual_quote_reserves`: `i128`
- `can_boost`: `bool`
- `base_supply`: `u64`
## event ClaimCashbackEvent disc=[226, 214, 246, 33, 7, 242, 147, 229]
- `user`: `pubkey`
- `amount`: `u64`
- `timestamp`: `i64`
- `total_claimed`: `u64`
- `total_cashback_earned`: `u64`
## event ClaimTokenIncentivesEvent disc=[79, 172, 246, 49, 205, 91, 206, 232]
- `user`: `pubkey`
- `mint`: `pubkey`
- `amount`: `u64`
- `timestamp`: `i64`
- `total_claimed_tokens`: `u64`
- `current_sol_volume`: `u64`
## event CloseUserVolumeAccumulatorEvent disc=[146, 159, 189, 172, 146, 88, 56, 244]
- `user`: `pubkey`
- `timestamp`: `i64`
- `total_unclaimed_tokens`: `u64`
- `total_claimed_tokens`: `u64`
- `current_sol_volume`: `u64`
- `last_update_timestamp`: `i64`
## event CollectCoinCreatorFeeEvent disc=[232, 245, 194, 238, 234, 218, 58, 89]
- `timestamp`: `i64`
- `coin_creator`: `pubkey`
- `coin_creator_fee`: `u64`
- `coin_creator_vault_ata`: `pubkey`
- `coin_creator_token_account`: `pubkey`
## event CreateConfigEvent disc=[107, 52, 89, 129, 55, 226, 81, 22]
- `timestamp`: `i64`
- `admin`: `pubkey`
- `lp_fee_basis_points`: `u64`
- `protocol_fee_basis_points`: `u64`
- `protocol_fee_recipients`: `{'array': ['pubkey', 8]}`
- `coin_creator_fee_basis_points`: `u64`
- `admin_set_coin_creator_authority`: `pubkey`
## event CreatePoolEvent disc=[177, 49, 12, 210, 160, 118, 167, 116]
- `timestamp`: `i64`
- `index`: `u16`
- `creator`: `pubkey`
- `base_mint`: `pubkey`
- `quote_mint`: `pubkey`
- `base_mint_decimals`: `u8`
- `quote_mint_decimals`: `u8`
- `base_amount_in`: `u64`
- `quote_amount_in`: `u64`
- `pool_base_amount`: `u64`
- `pool_quote_amount`: `u64`
- `minimum_liquidity`: `u64`
- `initial_liquidity`: `u64`
- `lp_token_amount_out`: `u64`
- `pool_bump`: `u8`
- `pool`: `pubkey`
- `lp_mint`: `pubkey`
- `user_base_token_account`: `pubkey`
- `user_quote_token_account`: `pubkey`
- `coin_creator`: `pubkey`
- `is_mayhem_mode`: `bool`
## event DepositEvent disc=[120, 248, 61, 83, 31, 142, 107, 144]
- `timestamp`: `i64`
- `lp_token_amount_out`: `u64`
- `max_base_amount_in`: `u64`
- `max_quote_amount_in`: `u64`
- `user_base_token_reserves`: `u64`
- `user_quote_token_reserves`: `u64`
- `pool_base_token_reserves`: `u64`
- `pool_quote_token_reserves`: `u64`
- `base_amount_in`: `u64`
- `quote_amount_in`: `u64`
- `lp_mint_supply`: `u64`
- `pool`: `pubkey`
- `user`: `pubkey`
- `user_base_token_account`: `pubkey`
- `user_quote_token_account`: `pubkey`
- `user_pool_token_account`: `pubkey`
## event DisableEvent disc=[107, 253, 193, 76, 228, 202, 27, 104]
- `timestamp`: `i64`
- `admin`: `pubkey`
- `disable_create_pool`: `bool`
- `disable_deposit`: `bool`
- `disable_withdraw`: `bool`
- `disable_buy`: `bool`
- `disable_sell`: `bool`
## event ExtendAccountEvent disc=[97, 97, 215, 144, 93, 146, 22, 124]
- `timestamp`: `i64`
- `account`: `pubkey`
- `user`: `pubkey`
- `current_size`: `u64`
- `new_size`: `u64`
## event InitBoostEvent disc=[174, 124, 74, 249, 4, 81, 246, 17]
- `timestamp`: `i64`
- `mint`: `pubkey`
- `bonding_curve`: `pubkey`
- `pool`: `pubkey`
- `virtual_quote_reserves`: `i128`
- `real_quote_reserves_after`: `u64`
## event InitUserVolumeAccumulatorEvent disc=[134, 36, 13, 72, 232, 101, 130, 216]
- `payer`: `pubkey`
- `user`: `pubkey`
- `timestamp`: `i64`
## event MigratePoolCoinCreatorEvent disc=[170, 221, 82, 199, 147, 165, 247, 46]
- `timestamp`: `i64`
- `base_mint`: `pubkey`
- `pool`: `pubkey`
- `sharing_config`: `pubkey`
- `old_coin_creator`: `pubkey`
- `new_coin_creator`: `pubkey`
## event ReservedFeeRecipientsEvent disc=[43, 188, 250, 18, 221, 75, 187, 95]
- `timestamp`: `i64`
- `reserved_fee_recipient`: `pubkey`
- `reserved_fee_recipients`: `{'array': ['pubkey', 7]}`
## event SellEvent disc=[62, 47, 55, 10, 165, 3, 220, 42]
- `timestamp`: `i64`
- `base_amount_in`: `u64`
- `min_quote_amount_out`: `u64`
- `user_base_token_reserves`: `u64`
- `user_quote_token_reserves`: `u64`
- `pool_base_token_reserves`: `u64`
- `pool_quote_token_reserves`: `u64`
- `quote_amount_out`: `u64`
- `lp_fee_basis_points`: `u64`
- `lp_fee`: `u64`
- `protocol_fee_basis_points`: `u64`
- `protocol_fee`: `u64`
- `quote_amount_out_without_lp_fee`: `u64`
- `user_quote_amount_out`: `u64`
- `pool`: `pubkey`
- `user`: `pubkey`
- `user_base_token_account`: `pubkey`
- `user_quote_token_account`: `pubkey`
- `protocol_fee_recipient`: `pubkey`
- `protocol_fee_recipient_token_account`: `pubkey`
- `coin_creator`: `pubkey`
- `coin_creator_fee_basis_points`: `u64`
- `coin_creator_fee`: `u64`
- `cashback_fee_basis_points`: `u64`
- `cashback`: `u64`
- `buyback_fee_basis_points`: `u64`
- `buyback_fee`: `u64`
- `virtual_quote_reserves`: `i128`
- `can_boost`: `bool`
- `base_supply`: `u64`
## event SetBondingCurveCoinCreatorEvent disc=[242, 231, 235, 102, 65, 99, 189, 211]
- `timestamp`: `i64`
- `base_mint`: `pubkey`
- `pool`: `pubkey`
- `bonding_curve`: `pubkey`
- `coin_creator`: `pubkey`
## event SetBoostAuthorityEvent disc=[89, 128, 240, 141, 91, 202, 71, 105]
- `timestamp`: `i64`
- `admin`: `pubkey`
- `old_boost_authority`: `pubkey`
- `new_boost_authority`: `pubkey`
## event SetMetaplexCoinCreatorEvent disc=[150, 107, 199, 123, 124, 207, 102, 228]
- `timestamp`: `i64`
- `base_mint`: `pubkey`
- `pool`: `pubkey`
- `metadata`: `pubkey`
- `coin_creator`: `pubkey`
## event SyncUserVolumeAccumulatorEvent disc=[197, 122, 167, 124, 116, 81, 91, 255]
- `user`: `pubkey`
- `total_claimed_tokens_before`: `u64`
- `total_claimed_tokens_after`: `u64`
- `timestamp`: `i64`
## event UpdateAdminEvent disc=[225, 152, 171, 87, 246, 63, 66, 234]
- `timestamp`: `i64`
- `admin`: `pubkey`
- `new_admin`: `pubkey`
## event UpdateFeeConfigEvent disc=[90, 23, 65, 35, 62, 244, 188, 208]
- `timestamp`: `i64`
- `admin`: `pubkey`
- `lp_fee_basis_points`: `u64`
- `protocol_fee_basis_points`: `u64`
- `protocol_fee_recipients`: `{'array': ['pubkey', 8]}`
- `coin_creator_fee_basis_points`: `u64`
- `admin_set_coin_creator_authority`: `pubkey`
## event WithdrawEvent disc=[22, 9, 133, 26, 160, 44, 71, 192]
- `timestamp`: `i64`
- `lp_token_amount_in`: `u64`
- `min_base_amount_out`: `u64`
- `min_quote_amount_out`: `u64`
- `user_base_token_reserves`: `u64`
- `user_quote_token_reserves`: `u64`
- `pool_base_token_reserves`: `u64`
- `pool_quote_token_reserves`: `u64`
- `base_amount_out`: `u64`
- `quote_amount_out`: `u64`
- `lp_mint_supply`: `u64`
- `pool`: `pubkey`
- `user`: `pubkey`
- `user_base_token_account`: `pubkey`
- `user_quote_token_account`: `pubkey`
- `user_pool_token_account`: `pubkey`
## account BondingCurve disc=[23, 183, 248, 55, 96, 216, 172, 96]
- `virtual_token_reserves`: `u64`
- `virtual_sol_reserves`: `u64`
- `real_token_reserves`: `u64`
- `real_sol_reserves`: `u64`
- `token_total_supply`: `u64`
- `complete`: `bool`
- `creator`: `pubkey`
- `is_mayhem_mode`: `bool`
- `is_cashback_coin`: `bool`
## account FeeConfig disc=[143, 52, 146, 187, 219, 123, 76, 155]
- `bump`: `u8`
- `admin`: `pubkey`
- `flat_fees`: `{'defined': {'name': 'Fees'}}`
- `fee_tiers`: `{'vec': {'defined': {'name': 'FeeTier'}}}`
- `stable_fee_tiers`: `{'vec': {'defined': {'name': 'FeeTier'}}}`
## account GlobalConfig disc=[149, 8, 156, 202, 160, 252, 176, 217]
- `admin`: `pubkey`
- `lp_fee_basis_points`: `u64`
- `protocol_fee_basis_points`: `u64`
- `disable_flags`: `u8`
- `protocol_fee_recipients`: `{'array': ['pubkey', 8]}`
- `coin_creator_fee_basis_points`: `u64`
- `admin_set_coin_creator_authority`: `pubkey`
- `whitelist_pda`: `pubkey`
- `reserved_fee_recipient`: `pubkey`
- `mayhem_mode_enabled`: `bool`
- `reserved_fee_recipients`: `{'array': ['pubkey', 7]}`
- `is_cashback_enabled`: `bool`
- `buyback_fee_recipients`: `{'array': ['pubkey', 8]}`
- `buyback_basis_points`: `u64`
- `boost_authority`: `pubkey`
- `boost_enabled`: `bool`
## account GlobalVolumeAccumulator disc=[202, 42, 246, 43, 142, 190, 30, 255]
- `start_time`: `i64`
- `end_time`: `i64`
- `seconds_in_a_day`: `i64`
- `mint`: `pubkey`
- `total_token_supply`: `{'array': ['u64', 30]}`
- `sol_volumes`: `{'array': ['u64', 30]}`
## account Pool disc=[241, 154, 109, 4, 17, 177, 109, 188]
- `pool_bump`: `u8`
- `index`: `u16`
- `creator`: `pubkey`
- `base_mint`: `pubkey`
- `quote_mint`: `pubkey`
- `lp_mint`: `pubkey`
- `pool_base_token_account`: `pubkey`
- `pool_quote_token_account`: `pubkey`
- `lp_supply`: `u64`
- `coin_creator`: `pubkey`
- `is_mayhem_mode`: `bool`
- `is_cashback_coin`: `bool`
- `virtual_quote_reserves`: `i128`
## account SharingConfig disc=[216, 74, 9, 0, 56, 140, 93, 75]
- `bump`: `u8`
- `version`: `u8`
- `status`: `{'defined': {'name': 'ConfigStatus'}}`
- `mint`: `pubkey`
- `admin`: `pubkey`
- `admin_revoked`: `bool`
- `shareholders`: `{'vec': {'defined': {'name': 'Shareholder'}}}`
## account UserVolumeAccumulator disc=[86, 255, 112, 14, 102, 53, 154, 250]
- `user`: `pubkey`
- `needs_claim`: `bool`
- `total_unclaimed_tokens`: `u64`
- `total_claimed_tokens`: `u64`
- `current_sol_volume`: `u64`
- `last_update_timestamp`: `i64`
- `has_total_claimed_tokens`: `bool`
- `cashback_earned`: `u64`
- `total_cashback_claimed`: `u64`

# pump_fees.json address=pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ
## event CreateFeeSharingConfigEvent disc=[133, 105, 170, 200, 184, 116, 251, 88]
- `timestamp`: `i64`
- `mint`: `pubkey`
- `bonding_curve`: `pubkey`
- `pool`: `{'option': 'pubkey'}`
- `sharing_config`: `pubkey`
- `admin`: `pubkey`
- `initial_shareholders`: `{'vec': {'defined': {'name': 'Shareholder'}}}`
- `status`: `{'defined': {'name': 'ConfigStatus'}}`
## event DonationFeePdaCranked disc=[30, 208, 107, 93, 177, 0, 223, 78]
- `timestamp`: `i64`
- `signer`: `pubkey`
- `donation_fee_pda`: `pubkey`
- `config_id`: `pubkey`
- `base_mint`: `pubkey`
- `quote_mint`: `pubkey`
- `creator`: `pubkey`
- `amount`: `u64`
## event DonationFeePdaCreated disc=[94, 20, 137, 239, 35, 77, 225, 235]
- `timestamp`: `i64`
- `created_by`: `pubkey`
- `donation_fee_pda`: `pubkey`
- `config_id`: `pubkey`
- `base_mint`: `pubkey`
- `quote_mint`: `pubkey`
- `creator`: `pubkey`
## event ExtendFeeConfigEvent disc=[226, 203, 224, 35, 153, 10, 88, 51]
- `fee_config`: `pubkey`
- `user`: `pubkey`
- `current_size`: `u64`
- `new_size`: `u64`
- `timestamp`: `i64`
## event InitializeFeeConfigEvent disc=[89, 138, 244, 230, 10, 56, 226, 126]
- `timestamp`: `i64`
- `admin`: `pubkey`
- `fee_config`: `pubkey`
## event InitializeFeeProgramGlobalEvent disc=[40, 233, 156, 78, 95, 0, 8, 199]
- `timestamp`: `i64`
- `authority`: `pubkey`
- `social_claim_authority`: `pubkey`
- `disable_flags`: `u8`
- `claim_rate_limit`: `u64`
## event ResetFeeSharingConfigEvent disc=[203, 204, 151, 226, 120, 55, 214, 243]
- `timestamp`: `i64`
- `mint`: `pubkey`
- `sharing_config`: `pubkey`
- `old_admin`: `pubkey`
- `old_shareholders`: `{'vec': {'defined': {'name': 'Shareholder'}}}`
- `new_admin`: `pubkey`
- `new_shareholders`: `{'vec': {'defined': {'name': 'Shareholder'}}}`
- `old_version`: `u8`
- `new_version`: `u8`
## event SetAuthorityEvent disc=[18, 175, 132, 66, 208, 201, 87, 242]
- `timestamp`: `i64`
- `old_authority`: `pubkey`
- `new_authority`: `pubkey`
## event SetClaimRateLimitEvent disc=[13, 143, 143, 235, 181, 19, 51, 40]
- `timestamp`: `i64`
- `claim_rate_limit`: `u64`
## event SetDisableFlagsEvent disc=[5, 8, 179, 65, 49, 55, 145, 126]
- `timestamp`: `i64`
- `disable_flags`: `u8`
## event SetSocialClaimAuthorityEvent disc=[60, 118, 127, 132, 239, 52, 254, 14]
- `timestamp`: `i64`
- `social_claim_authority`: `pubkey`
## event SocialFeePdaClaimed disc=[50, 18, 193, 65, 237, 210, 234, 236]
- `timestamp`: `i64`
- `user_id`: `string`
- `platform`: `u8`
- `social_fee_pda`: `pubkey`
- `recipient`: `pubkey`
- `social_claim_authority`: `pubkey`
- `amount_claimed`: `u64`
- `claimable_before`: `u64`
- `lifetime_claimed`: `u64`
- `recipient_balance_before`: `u64`
- `recipient_balance_after`: `u64`
- `quote_mint`: `pubkey`
- `lifetime_stable_claimed`: `u64`
## event SocialFeePdaCreated disc=[183, 183, 218, 147, 24, 124, 137, 169]
- `timestamp`: `i64`
- `user_id`: `string`
- `platform`: `u8`
- `social_fee_pda`: `pubkey`
- `created_by`: `pubkey`
## event SweepBuybackEvent disc=[43, 56, 42, 214, 153, 57, 166, 137]
- `index`: `u8`
- `sol_amount`: `u64`
- `token_amount`: `u64`
- `destination`: `pubkey`
- `buyback_vault`: `pubkey`
- `mint`: `pubkey`
## event UpdateAdminEvent disc=[225, 152, 171, 87, 246, 63, 66, 234]
- `timestamp`: `i64`
- `old_admin`: `pubkey`
- `new_admin`: `pubkey`
## event UpdateFeeConfigEvent disc=[90, 23, 65, 35, 62, 244, 188, 208]
- `timestamp`: `i64`
- `admin`: `pubkey`
- `fee_config`: `pubkey`
- `fee_tiers`: `{'vec': {'defined': {'name': 'FeeTier'}}}`
- `flat_fees`: `{'defined': {'name': 'Fees'}}`
## event UpdateFeeSharesEvent disc=[21, 186, 196, 184, 91, 228, 225, 203]
- `timestamp`: `i64`
- `mint`: `pubkey`
- `sharing_config`: `pubkey`
- `admin`: `pubkey`
- `new_shareholders`: `{'vec': {'defined': {'name': 'Shareholder'}}}`
- `version`: `u8`
## event UpdateStableFeeConfigEvent disc=[94, 5, 43, 237, 103, 147, 232, 245]
- `timestamp`: `i64`
- `admin`: `pubkey`
- `fee_config`: `pubkey`
- `stable_fee_tiers`: `{'vec': {'defined': {'name': 'FeeTier'}}}`
- `flat_fees`: `{'defined': {'name': 'Fees'}}`
## event UpsertFeeTiersEvent disc=[171, 89, 169, 187, 122, 186, 33, 204]
- `timestamp`: `i64`
- `admin`: `pubkey`
- `fee_config`: `pubkey`
- `fee_tiers`: `{'vec': {'defined': {'name': 'FeeTier'}}}`
- `offset`: `u8`
## event UpsertStableFeeTiersEvent disc=[232, 237, 237, 52, 98, 146, 73, 243]
- `timestamp`: `i64`
- `admin`: `pubkey`
- `fee_config`: `pubkey`
- `stable_fee_tiers`: `{'vec': {'defined': {'name': 'FeeTier'}}}`
- `offset`: `u8`
## account BondingCurve disc=[23, 183, 248, 55, 96, 216, 172, 96]
- `virtual_token_reserves`: `u64`
- `virtual_quote_reserves`: `u64`
- `real_token_reserves`: `u64`
- `real_quote_reserves`: `u64`
- `token_total_supply`: `u64`
- `complete`: `bool`
- `creator`: `pubkey`
- `is_mayhem_mode`: `bool`
## account BuybackVault disc=[153, 166, 71, 144, 179, 189, 137, 251]
- `authority`: `pubkey`
- `total_claimed`: `u64`
- `total_claimed_token1`: `u64`
- `total_claimed_token2`: `u64`
- `last_claimed`: `i64`
- `claim_rate_limit`: `i64`
- `_reserved`: `{'array': ['u8', 128]}`
## account DonationFeePda disc=[246, 197, 96, 9, 193, 30, 93, 115]
- `bump`: `u8`
- `version`: `u8`
- `config_id`: `pubkey`
- `base_mint`: `pubkey`
- `quote_mint`: `pubkey`
- `creator`: `pubkey`
- `total_donated`: `u64`
- `last_crank_ts`: `i64`
- `_reserved`: `{'array': ['u8', 64]}`
## account FeeConfig disc=[143, 52, 146, 187, 219, 123, 76, 155]
- `bump`: `u8`
- `admin`: `pubkey`
- `flat_fees`: `{'defined': {'name': 'Fees'}}`
- `fee_tiers`: `{'vec': {'defined': {'name': 'FeeTier'}}}`
- `stable_fee_tiers`: `{'vec': {'defined': {'name': 'FeeTier'}}}`
## account FeeProgramGlobal disc=[162, 165, 245, 49, 29, 37, 55, 242]
- `bump`: `u8`
- `authority`: `pubkey`
- `disable_flags`: `u8`
- `social_claim_authority`: `pubkey`
- `claim_rate_limit`: `u64`
- `_reserved`: `{'array': ['u8', 256]}`
## account Global disc=[167, 232, 232, 177, 200, 108, 114, 127]
- `initialized`: `bool`
- `authority`: `pubkey`
- `fee_recipient`: `pubkey`
- `initial_virtual_token_reserves`: `u64`
- `initial_virtual_sol_reserves`: `u64`
- `initial_real_token_reserves`: `u64`
- `token_total_supply`: `u64`
- `fee_basis_points`: `u64`
- `withdraw_authority`: `pubkey`
- `enable_migrate`: `bool`
- `pool_migration_fee`: `u64`
- `creator_fee_basis_points`: `u64`
- `fee_recipients`: `{'array': ['pubkey', 7]}`
- `set_creator_authority`: `pubkey`
- `admin_set_creator_authority`: `pubkey`
- `create_v2_enabled`: `bool`
- `whitelist_pda`: `pubkey`
- `reserved_fee_recipient`: `pubkey`
- `mayhem_mode_enabled`: `bool`
- `reserved_fee_recipients`: `{'array': ['pubkey', 7]}`
## account Pool disc=[241, 154, 109, 4, 17, 177, 109, 188]
- `pool_bump`: `u8`
- `index`: `u16`
- `creator`: `pubkey`
- `base_mint`: `pubkey`
- `quote_mint`: `pubkey`
- `lp_mint`: `pubkey`
- `pool_base_token_account`: `pubkey`
- `pool_quote_token_account`: `pubkey`
- `lp_supply`: `u64`
- `coin_creator`: `pubkey`
- `is_mayhem_mode`: `bool`
## account SharingConfig disc=[216, 74, 9, 0, 56, 140, 93, 75]
- `bump`: `u8`
- `version`: `u8`
- `status`: `{'defined': {'name': 'ConfigStatus'}}`
- `mint`: `pubkey`
- `admin`: `pubkey`
- `admin_revoked`: `bool`
- `shareholders`: `{'vec': {'defined': {'name': 'Shareholder'}}}`
## account SocialFeePda disc=[139, 96, 53, 17, 42, 169, 206, 150]
- `bump`: `u8`
- `version`: `u8`
- `user_id`: `string`
- `platform`: `u8`
- `total_claimed`: `u64`
- `last_claimed`: `u64`
- `total_stable_claimed`: `u64`
- `_reserved`: `{'array': ['u8', 120]}`
