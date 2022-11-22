#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::traits::Currency;
pub use pallet::*;

type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

#[frame_support::pallet]
pub mod pallet {
	use core::{
		cmp::min,
		ops::{Mul, Sub},
	};
	use frame_support::{
		dispatch::HasCompact,
		pallet_prelude::*,
		sp_runtime::traits::{AccountIdConversion, IntegerSquareRoot, Zero},
		traits::fungibles::{Create, Inspect, Mutate, Transfer},
		PalletId,
	};
	use frame_support::traits::ReservableCurrency;
	use frame_system::pallet_prelude::*;
	use sp_runtime::{Percent, SaturatedConversion};
	use crate::BalanceOf;

	const MODULE_ID: PalletId = PalletId(*b"subswap0");
	const TREASURY_ID: PalletId = PalletId(*b"treasury");

	#[pallet::pallet]
	#[pallet::generate_store(pub (super) trait Store)]
	pub struct Pallet<T>(_);

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config{
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type GovernanceOrigin: EnsureOrigin<Self::RuntimeOrigin>;
		type Currency: ReservableCurrency<Self::AccountId>;

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
				Balance = BalanceOf<Self>,
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
		type MinimumLiquidity: Get<BalanceOf<Self>>;
	}

	#[pallet::storage]
	#[pallet::getter(fn pool_reserves)]
	pub type PoolReserves<T: Config> = StorageMap<_, Blake2_128Concat, T::AssetId, BalanceOf<T>>;

	#[pallet::event]
	#[pallet::generate_deposit(pub (super) fn deposit_event)]
	pub enum Event<T: Config> {
		Mint {
			sender: T::AccountId,
			token_a_amount: BalanceOf<T>,
			token_b_amount: BalanceOf<T>,
			liquidity: BalanceOf<T>,
		},
		Burn {
			to: T::AccountId,
			token_a_amount: BalanceOf<T>,
			token_b_amount: BalanceOf<T>,
		},
		AlterPoolDistribution {
			prev_reserve_a: BalanceOf<T>,
			prev_reserve_b: BalanceOf<T>,
			reserve_a: BalanceOf<T>,
			reserve_b: BalanceOf<T>,
		},
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		NoneValue,
		StorageOverflow,
		PoolNotInitialized,
		PoolAlreadyInitialized,
		InsufficientAAmount,
		InsufficientBAmount,
		RationErrorMustBeBetween0to1,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(10_000)]
		pub fn initialize_pool(origin: OriginFor<T>) -> DispatchResult {
			T::GovernanceOrigin::ensure_origin(origin.clone())?;
			// Ensure that the Pool is not already initialized
			let (token_a, token_b) = Self::token_pair();
			ensure!(!<PoolReserves<T>>::contains_key(token_a), Error::<T>::PoolAlreadyInitialized);
			ensure!(!<PoolReserves<T>>::contains_key(token_b), Error::<T>::PoolAlreadyInitialized);
			// Initialize Pool Reserves
			<PoolReserves<T>>::insert(T::TokenA::get(), BalanceOf::<T>::zero());
			<PoolReserves<T>>::insert(T::TokenB::get(), BalanceOf::<T>::zero());
			Ok(())
		}

		#[pallet::weight(10_000)]
		pub fn add_liquidity(
			origin: OriginFor<T>,
			token_a_amount: BalanceOf<T>,
			token_b_amount: BalanceOf<T>,
			token_a_min_amount: BalanceOf<T>,
			token_b_min_amount: BalanceOf<T>,
		) -> DispatchResult {
			let (token_a, token_b) = Self::token_pair();
			ensure!(<PoolReserves<T>>::contains_key(token_a), Error::<T>::PoolNotInitialized);
			ensure!(<PoolReserves<T>>::contains_key(token_b), Error::<T>::PoolNotInitialized);
			let sender = ensure_signed(origin)?;
			let (token_a_amount, token_b_amount) = Self::_add_liquidity(
				token_a_amount,
				token_b_amount,
				token_a_min_amount,
				token_b_min_amount,
			)?;
			T::AssetsManager::can_withdraw(token_a, &sender, token_a_amount).into_result()?;
			T::AssetsManager::can_withdraw(token_b, &sender, token_b_amount).into_result()?;
			T::AssetsManager::transfer(
				token_a,
				&sender,
				&Self::fund_account_id(),
				token_a_amount,
				true,
			)?;
			T::AssetsManager::transfer(
				token_b,
				&sender,
				&Self::fund_account_id(),
				token_b_amount,
				true,
			)?;
			Self::mint(sender)
		}

		#[pallet::weight(10_000)]
		pub fn remove_liquidity(origin: OriginFor<T>, liquidity: BalanceOf<T>) -> DispatchResult {
			let (token_a, token_b) = Self::token_pair();
			ensure!(<PoolReserves<T>>::contains_key(token_a), Error::<T>::PoolNotInitialized);
			ensure!(<PoolReserves<T>>::contains_key(token_b), Error::<T>::PoolNotInitialized);
			let sender = ensure_signed(origin)?;
			T::AssetsManager::transfer(
				Self::pair_token(),
				&sender,
				&Self::fund_account_id(),
				liquidity,
				true,
			)?;
			Self::burn(sender)
		}

		#[pallet::weight(10_000)]
		pub fn change_pool_distribution(
			origin: OriginFor<T>,
			ratio_token_a_per_token_b: Percent,
		) -> DispatchResult {
			T::GovernanceOrigin::ensure_origin(origin.clone())?;
			let (token_a, token_b) = Self::token_pair();
			ensure!(<PoolReserves<T>>::contains_key(token_a), Error::<T>::PoolNotInitialized);
			ensure!(<PoolReserves<T>>::contains_key(token_b), Error::<T>::PoolNotInitialized);
			let token_a_reserve =
				<PoolReserves<T>>::get(&token_a).ok_or(Error::<T>::PoolNotInitialized)?;
			let token_b_reserve =
				<PoolReserves<T>>::get(&token_b).ok_or(Error::<T>::PoolNotInitialized)?;
			let fund_account = Self::fund_account_id();
			let new_token_a_reserve : BalanceOf<T> = ratio_token_a_per_token_b.mul_ceil::<u128>((token_a_reserve + token_b_reserve).saturated_into::<u128>()).saturated_into();
			if new_token_a_reserve > token_a_reserve {
				let diff = new_token_a_reserve - token_a_reserve;
				T::AssetsManager::burn_from(token_b, &fund_account, diff)?;
				T::AssetsManager::mint_into(token_a, &fund_account, diff)?;
			} else if new_token_a_reserve < token_a_reserve {
				let diff = token_a_reserve - new_token_a_reserve;
				T::AssetsManager::burn_from(token_a, &fund_account, diff)?;
				T::AssetsManager::mint_into(token_b, &fund_account, diff)?;
			}

			let token_a_balance = T::AssetsManager::balance(token_a, &fund_account);
			let token_b_balance = T::AssetsManager::balance(token_b, &fund_account);

			<PoolReserves<T>>::insert(token_a, token_a_balance);
			<PoolReserves<T>>::insert(token_b, token_b_balance);
			Self::deposit_event(Event::<T>::AlterPoolDistribution {
				prev_reserve_a: token_a_reserve,
				prev_reserve_b: token_b_reserve,
				reserve_a: token_a_balance,
				reserve_b: token_b_balance,
			});
			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		pub fn fund_account_id() -> T::AccountId {
			MODULE_ID.into_account_truncating()
		}
		pub fn treasury_account_id() -> T::AccountId {
			TREASURY_ID.into_account_truncating()
		}

		pub fn token_pair() -> (T::AssetId, T::AssetId) {
			(T::TokenA::get(), T::TokenB::get())
		}

		pub fn pair_token() -> T::AssetId {
			T::PoolToken::get()
		}

		pub fn minimum_liquidity() -> BalanceOf<T> {
			T::MinimumLiquidity::get()
		}

		fn _add_liquidity(
			token_a_amount: BalanceOf<T>,
			token_b_amount: BalanceOf<T>,
			token_a_min_amount: BalanceOf<T>,
			token_b_min_amount: BalanceOf<T>,
		) -> Result<(BalanceOf<T>, BalanceOf<T>), DispatchError> {
			let (token_a, token_b) = Self::token_pair();

			let token_a_reserve =
				<PoolReserves<T>>::get(&token_a).ok_or(Error::<T>::PoolNotInitialized)?;
			let token_b_reserve =
				<PoolReserves<T>>::get(&token_b).ok_or(Error::<T>::PoolNotInitialized)?;
			return if token_a_reserve.is_zero() && token_b_reserve.is_zero() {
				Ok((token_a_amount, token_b_amount))
			} else {
				let amount_b_optimal =
					Self::quote(token_a_amount, token_a_reserve, token_b_reserve);
				if amount_b_optimal <= token_b_amount {
					ensure!(
						amount_b_optimal >= token_b_min_amount,
						Error::<T>::InsufficientBAmount
					);
					Ok((token_a_amount, amount_b_optimal))
				} else {
					let amount_a_optimal =
						Self::quote(token_b_amount, token_b_reserve, token_a_reserve);
					ensure!(
						amount_a_optimal >= token_a_min_amount,
						Error::<T>::InsufficientAAmount
					);
					Ok((amount_a_optimal, token_b_amount))
				}
			}
		}

		fn quote(amount_a: BalanceOf<T>, reserve_a: BalanceOf<T>, reserve_b: BalanceOf<T>) -> BalanceOf<T> {
			amount_a.mul(reserve_b) / reserve_a
		}

		fn mint(to: T::AccountId) -> DispatchResult {
			let (token_a, token_b) = Self::token_pair();
			let fund_account = Self::fund_account_id();
			let token_a_balance = T::AssetsManager::balance(token_a, &fund_account);
			let token_b_balance = T::AssetsManager::balance(token_b, &fund_account);

			let token_a_reserve =
				<PoolReserves<T>>::get(&token_a).ok_or(Error::<T>::PoolNotInitialized)?;
			let token_b_reserve =
				<PoolReserves<T>>::get(&token_b).ok_or(Error::<T>::PoolNotInitialized)?;
			let amount_a = token_a_balance.sub(token_a_reserve);
			let amount_b = token_b_balance.sub(token_b_reserve);

			let total_supply = T::AssetsManager::total_issuance(Self::pair_token());
			let liquidity = if total_supply.is_zero() {
				// Lock Minimum LP tokens
				T::AssetsManager::mint_into(
					Self::pair_token(),
					&Self::treasury_account_id(),
					T::MinimumLiquidity::get(),
				)?;
				(amount_a * amount_b).integer_sqrt().sub(T::MinimumLiquidity::get())
			} else {
				min(
					amount_a.mul(total_supply) / token_a_reserve,
					amount_b.mul(total_supply) / token_b_reserve,
				)
			};

			T::AssetsManager::mint_into(T::PoolToken::get(), &to, liquidity)?;
			<PoolReserves<T>>::insert(token_a, token_a_balance);
			<PoolReserves<T>>::insert(token_b, token_b_balance);
			Self::deposit_event(Event::<T>::Mint {
				sender: to,
				token_a_amount: amount_a,
				token_b_amount: amount_b,
				liquidity,
			});
			Ok(())
		}

		fn burn(to: T::AccountId) -> DispatchResult {
			let fund_account = Self::fund_account_id();

			let (token_a, token_b) = Self::token_pair();

			let token_a_balance = T::AssetsManager::balance(token_a, &fund_account);
			let token_b_balance = T::AssetsManager::balance(token_b, &fund_account);

			let total_supply = T::AssetsManager::total_issuance(Self::pair_token());
			let liquidity = T::AssetsManager::balance(Self::pair_token(), &fund_account);
			// Get the total share of `token a` per liquidity pool token share
			let amount_a = liquidity.mul(token_a_balance) / total_supply;
			// Get the total share of `token b` per liquidity pool token share
			let amount_b = liquidity.mul(token_b_balance) / total_supply;

			T::AssetsManager::burn_from(Self::pair_token(), &Self::fund_account_id(), liquidity)?;

			T::AssetsManager::transfer(token_a, &fund_account, &to, amount_a, false)?;
			T::AssetsManager::transfer(token_b, &fund_account, &to, amount_b, false)?;

			let token_a_balance = T::AssetsManager::balance(token_a, &fund_account);
			let token_b_balance = T::AssetsManager::balance(token_b, &fund_account);

			<PoolReserves<T>>::insert(token_a, token_a_balance);
			<PoolReserves<T>>::insert(token_b, token_b_balance);

			Self::deposit_event(Event::<T>::Burn {
				to,
				token_a_amount: amount_a,
				token_b_amount: amount_b,
			});
			Ok(())
		}
	}
}
