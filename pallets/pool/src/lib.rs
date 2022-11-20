pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::{
		dispatch::HasCompact,
		pallet_prelude::*,
		sp_runtime::traits::{AccountIdConversion, CheckedDiv, IntegerSquareRoot, Zero},
		traits::fungibles::{Create, Inspect, Mutate, Transfer},
		PalletId,
	};
	use frame_system::pallet_prelude::*;
	use std::{
		cmp::min,
		ops::{Mul, Sub},
	};

	const MODULE_ID: PalletId = PalletId(*b"subswap0");

	#[pallet::pallet]
	#[pallet::generate_store(pub (super) trait Store)]
	pub struct Pallet<T>(_);

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_balances::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type GovernanceOrigin: EnsureOrigin<
			Self::RuntimeOrigin,
			Success = <Self as frame_system::Config>::AccountId,
		>;

		type AssetId: Member
			+ Parameter
			+ Default
			+ Copy
			+ HasCompact
			+ MaybeSerializeDeserialize
			+ MaxEncodedLen
			+ TypeInfo;
		type AssetsManager: Create<<Self as frame_system::Config>::AccountId>
			+ Mutate<
				<Self as frame_system::Config>::AccountId,
				Balance = Self::Balance,
				AssetId = Self::AssetId,
			> + Inspect<<Self as frame_system::Config>::AccountId>
			+ Transfer<<Self as frame_system::Config>::AccountId>;

		#[pallet::constant]
		type PoolToken: Get<Self::AssetId>;
		#[pallet::constant]
		type TokenA: Get<Self::AssetId>;
		#[pallet::constant]
		type TokenB: Get<Self::AssetId>;
		#[pallet::constant]
		type MinimumLiquidity: Get<Self::Balance>;
	}

	#[pallet::storage]
	#[pallet::getter(fn pool_reserves)]
	pub type PoolReserves<T: Config> = StorageMap<_, Blake2_128Concat, T::AssetId, T::Balance>;

	#[pallet::event]
	#[pallet::generate_deposit(pub (super) fn deposit_event)]
	pub enum Event<T: Config> {}

	// #[pallet::genesis_build]
	// impl<T: Config> GenesisBuild<T> for GenesisConfig {
	// 	fn build(&self) {}
	// }

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		NoneValue,
		StorageOverflow,
		PoolNotInitialized,
		PoolAlreadyInitialized,
		Overflow,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(10_000)]
		pub fn initialize_pool(
			origin: OriginFor<T>,
			token_a_reserves: T::Balance,
			token_b_reserves: T::Balance,
		) -> DispatchResult {
            let fund_manager = T::GovernanceOrigin::ensure_origin(origin)?;
			// Ensure that the Pool is not already initialized
			ensure!(
				!<PoolReserves<T>>::contains_key(T::TokenA::get()),
				Error::<T>::PoolAlreadyInitialized
			);
			ensure!(
				!<PoolReserves<T>>::contains_key(T::TokenB::get()),
				Error::<T>::PoolAlreadyInitialized
			);
			// Initialize Pool Reserves
			<PoolReserves<T>>::insert(T::TokenA::get(), T::Balance::zero());
			<PoolReserves<T>>::insert(T::TokenB::get(), T::Balance::zero());
			T::AssetsManager::can_withdraw(T::TokenA::get(), &fund_manager, token_a_reserves);
			T::AssetsManager::can_withdraw(T::TokenB::get(), &fund_manager, token_b_reserves);

			// Add tokens to fund account
			T::AssetsManager::transfer(
				T::TokenA::get(),
				&fund_manager,
				&Self::fund_account_id(),
				token_a_reserves,
				false,
			)?;
			T::AssetsManager::transfer(
				T::TokenB::get(),
				&fund_manager,
				&Self::fund_account_id(),
				token_b_reserves,
				false,
			)?;
			// Mint LP tokens into fund manager account
			Self::mint(fund_manager)
		}

		#[pallet::weight(10_000)]
		pub fn add_liquidity(
			origin: OriginFor<T>,
			token_a_amount: T::Balance,
			token_b_amount: T::Balance,
			token_a_min_amount: T::Balance,
			token_b_min_amount: T::Balance,
		) -> DispatchResult {
			todo!()
		}
	}

	impl<T: Config> Pallet<T> {
		pub fn fund_account_id() -> T::AccountId {
			MODULE_ID.into_account_truncating()
		}

		pub fn token_a() -> T::AssetId {
			T::TokenA::get()
		}

		pub fn token_b() -> T::AssetId {
			T::TokenA::get()
		}
		pub fn pair_token() -> T::AssetId {
			T::PoolToken::get()
		}

		pub fn minimum_liquidity() -> T::Balance {
			T::MinimumLiquidity::get()
		}

		pub fn _add_liquidity(
			origin: OriginFor<T>,
			token_a_amount: T::Balance,
			token_b_amount: T::Balance,
			token_a_min_amount: T::Balance,
			token_b_min_amount: T::Balance,
		) -> DispatchResult {
			todo!()
		}

		fn mint(to: T::AccountId) -> DispatchResult {
			let fund_account = Self::fund_account_id();
			let token_a_balance = T::AssetsManager::balance(Self::token_a(), &fund_account);
			let token_b_balance = T::AssetsManager::balance(Self::token_b(), &fund_account);

			let token_a_reserve =
				<PoolReserves<T>>::get(&Self::token_a()).ok_or(Error::<T>::PoolNotInitialized)?;
			let token_b_reserve =
				<PoolReserves<T>>::get(&Self::token_b()).ok_or(Error::<T>::PoolNotInitialized)?;
			let amount_a = token_a_balance.sub(token_a_reserve);
			let amount_b = token_b_balance.sub(token_b_reserve);

			let total_supply = T::AssetsManager::total_issuance(Self::pair_token());
			let liquidity = if total_supply.is_zero() {
				// Mint Minimum LP tokens
				// T::AssetsManager::mint_into(Self::pair_token(), 0_i32.into_account_truncating(),
				// T::MinimumLiquidity::get())?;
				(amount_a * amount_b).integer_sqrt().sub(T::MinimumLiquidity::get())
			} else {
				min(
					amount_a.mul(total_supply) / token_a_reserve,
					amount_b.mul(total_supply) / token_b_reserve,
				)
			};

			T::AssetsManager::mint_into(T::PoolToken::get(), &to, liquidity)?;
			<PoolReserves<T>>::insert(Self::token_a(), token_a_balance);
			<PoolReserves<T>>::insert(Self::token_b(), token_b_balance);
			Ok(())
		}

		fn burn(to: T::AccountId) -> DispatchResult {
			let fund_account = Self::fund_account_id();

			let token_a_balance = T::AssetsManager::balance(Self::token_a(), &fund_account);
			let token_b_balance = T::AssetsManager::balance(Self::token_b(), &fund_account);

			let total_supply = T::AssetsManager::total_issuance(Self::pair_token());
			let liquidity = T::AssetsManager::balance(Self::pair_token(), &fund_account);

			let amount_a = liquidity
				.mul(token_a_balance)
				.checked_div(&total_supply)
				.ok_or(Error::<T>::Overflow)?;
			let amount_b = liquidity
				.mul(token_b_balance)
				.checked_div(&total_supply)
				.ok_or(Error::<T>::Overflow)?;

			T::AssetsManager::burn_from(Self::pair_token(), &Self::fund_account_id(), liquidity)?;

			T::AssetsManager::transfer(
				Self::token_a(),
				&Self::fund_account_id(),
				&to,
				amount_a,
				false,
			)?;
			T::AssetsManager::transfer(
				Self::token_b(),
				&Self::fund_account_id(),
				&to,
				amount_b,
				false,
			)?;

			let token_a_balance = T::AssetsManager::balance(Self::token_a(), &fund_account);
			let token_b_balance = T::AssetsManager::balance(Self::token_b(), &fund_account);

			<PoolReserves<T>>::insert(Self::token_a(), token_a_balance);
			<PoolReserves<T>>::insert(Self::token_b(), token_b_balance);

			Ok(())
		}
	}
}
